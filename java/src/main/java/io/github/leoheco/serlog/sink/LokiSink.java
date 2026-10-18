package io.github.leoheco.serlog.sink;

import static io.github.leoheco.serlog.layout.Text.quote;

import io.github.leoheco.serlog.Entry;
import io.github.leoheco.serlog.Field;
import io.github.leoheco.serlog.Queued;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.SinkBase;
import io.github.leoheco.serlog.internal.Hub;
import io.github.leoheco.serlog.internal.Units;
import io.github.leoheco.serlog.layout.Layout;
import io.github.leoheco.serlog.layout.PatternLayout;
import io.github.leoheco.serlog.layout.Text;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;
import java.util.stream.Collectors;
import java.util.zip.GZIPOutputStream;

/// Grafana Loki's push API, which Alloy's `loki.source.api` accepts as well: gzipped JSON, one stream per level.
/// Lines are the slot's layout, `{msg}` unless set; `target`, `file` and the fields travel as structured metadata.
public final class LokiSink extends SinkBase<LokiSink> {
    private final String url;
    private Map<String, String> labels = Map.of();
    private Duration timeout = Duration.ofSeconds(10);
    private boolean connect;
    private HttpClient client;

    /// Pushes to `url`, such as `http://alloy:3100/loki/api/v1/push`.
    public LokiSink(String url) {
        this.url = url;
    }

    /// Stream labels besides `app`, `env`, `instance` and `level`.
    public LokiSink labels(Map<String, String> labels) {
        this.labels = new LinkedHashMap<>(labels);
        return this;
    }

    /// How long a request may take, 10 s by default.
    public LokiSink timeout(Duration timeout) {
        this.timeout = timeout;
        return this;
    }

    /// Whether to ask `GET /ready` at start-up.
    public LokiSink connect(boolean connect) {
        this.connect = connect;
        return this;
    }

    @Override
    public String kind() {
        return "loki";
    }

    private static String object(List<Map.Entry<String, String>> pairs) {
        return pairs.stream()
                .map(p -> quote(p.getKey()) + ":" + quote(p.getValue()))
                .collect(Collectors.joining(",", "{", "}"));
    }

    /// The push request body: labels per stream, structured metadata per line.
    public String body(List<Entry> entries) {
        var m = Hub.meta();
        var streams = new TreeMap<Integer, List<Entry>>();
        for (var e : entries)
            streams.computeIfAbsent(e.record().level().value(), _ -> new ArrayList<>())
                    .add(e);
        var out = new ArrayList<String>();
        streams.forEach((level, list) -> {
            var base = new ArrayList<Map.Entry<String, String>>();
            for (var id :
                    List.of(Map.entry("app", m.app()), Map.entry("env", m.env()), Map.entry("instance", m.instance())))
                if (!id.getValue().isEmpty()) base.add(id);
            base.add(Map.entry("level", list.getFirst().record().level().id()));
            base.addAll(labels.entrySet());
            var values = list.stream().map(e -> {
                var r = e.record();
                var ns = r.time().getEpochSecond() * 1_000_000_000L + r.time().getNano();
                var extra = new ArrayList<Map.Entry<String, String>>();
                extra.add(Map.entry("target", r.target()));
                if (r.file() != null && r.line() > 0) extra.add(Map.entry("file", r.file() + ":" + r.line()));
                for (Field f : Text.flat(r.fields(), java.util.Set.of()))
                    extra.add(Map.entry(f.key(), Text.show(f.value())));
                return "[\"" + ns + "\"," + quote(e.line()) + "," + object(extra) + "]";
            });
            out.add("{\"stream\":" + object(base) + ",\"values\":[" + values.collect(Collectors.joining(",")) + "]}");
        });
        return "{\"streams\":[" + String.join(",", out) + "]}";
    }

    private synchronized HttpClient client() {
        if (client == null)
            client = HttpClient.newBuilder().connectTimeout(timeout).build();
        return client;
    }

    @Override
    public void emit(Entry entry) throws IOException, InterruptedException {
        emitBatch(List.of(entry));
    }

    @Override
    public void emitBatch(List<Entry> entries) throws IOException, InterruptedException {
        if (entries.isEmpty()) return;
        var gz = new ByteArrayOutputStream();
        try (var out = new GZIPOutputStream(gz)) {
            out.write(body(entries).getBytes(StandardCharsets.UTF_8));
        }
        var req = HttpRequest.newBuilder(URI.create(url))
                .timeout(timeout)
                .header("Content-Type", "application/json")
                .header("Content-Encoding", "gzip")
                .POST(HttpRequest.BodyPublishers.ofByteArray(gz.toByteArray()))
                .build();
        var status = client().send(req, HttpResponse.BodyHandlers.discarding()).statusCode();
        if (status / 100 == 2) return;
        if (status >= 400 && status < 500 && status != 429)
            throw new SerlogException.Rejected("loki answered " + status);
        throw new IOException("loki answered " + status);
    }

    @Override
    public void ready() throws IOException, InterruptedException {
        if (!url.startsWith("http://") && !url.startsWith("https://"))
            throw new SerlogException.Config("loki url `" + url + "` is not http or https");
        if (!connect) return;
        var i = url.indexOf("/loki/api/");
        var base = i >= 0 ? url.substring(0, i) : url.replaceAll("/+$", "");
        var req = HttpRequest.newBuilder(URI.create(base + "/ready"))
                .timeout(timeout)
                .GET()
                .build();
        var status = client().send(req, HttpResponse.BodyHandlers.discarding()).statusCode();
        if (status / 100 != 2) throw new IOException("loki /ready answered " + status);
    }

    @Override
    public Optional<Layout> defaultLayout() {
        return Optional.of(new PatternLayout("{msg}"));
    }

    @Override
    public Optional<Queued> queued() {
        return Optional.of(Queued.of(8192));
    }

    @Override
    public Optional<Map<String, Object>> options() {
        var o = new LinkedHashMap<String, Object>();
        o.put("url", url);
        if (!labels.isEmpty()) o.put("labels", new LinkedHashMap<>(labels));
        if (!timeout.equals(Duration.ofSeconds(10))) o.put("timeout", Units.showTime(timeout));
        if (connect) o.put("connect", true);
        return Optional.of(o);
    }
}
