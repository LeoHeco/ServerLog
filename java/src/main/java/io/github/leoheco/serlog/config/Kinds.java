package io.github.leoheco.serlog.config;

import io.github.leoheco.serlog.Compat;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.Sink;
import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.alarm.Tiers;
import io.github.leoheco.serlog.layout.Layout;
import io.github.leoheco.serlog.process.Dedupe;
import io.github.leoheco.serlog.process.Fields;
import io.github.leoheco.serlog.process.Limit;
import io.github.leoheco.serlog.process.Match;
import io.github.leoheco.serlog.process.Processor;
import io.github.leoheco.serlog.process.Redact;
import io.github.leoheco.serlog.process.Sample;
import io.github.leoheco.serlog.process.Sanitise;
import io.github.leoheco.serlog.sink.ConsoleSink;
import io.github.leoheco.serlog.sink.FileSink;
import io.github.leoheco.serlog.sink.LokiSink;
import io.github.leoheco.serlog.sink.MailSink;
import io.github.leoheco.serlog.sink.MemorySink;
import java.time.Duration;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Function;
import java.util.function.Supplier;

/// The four kinds of extension a config file can name: sinks (`kind`), processors (a `process` entry), layouts
/// (`layout: name`) and alarms (`kind` under `alarms`). A factory receives the entry's own keys and must read them
/// all.
public final class Kinds {
    private static final Map<String, Function<Options, Sink>> SINKS = new ConcurrentHashMap<>();
    private static final Map<String, Function<Options, Processor>> STEPS = new ConcurrentHashMap<>();
    private static final Map<String, Supplier<Layout>> LAYOUTS = new ConcurrentHashMap<>();
    private static final Map<String, Function<Options, Alarm>> ALARMS = new ConcurrentHashMap<>();

    private Kinds() {}

    static {
        SINKS.put("console", o -> {
            var s = new ConsoleSink();
            o.string("stream", null).ifPresent(v -> s.stream(Compat.Output.parse(v)));
            return s;
        });
        SINKS.put("file", Kinds::file);
        SINKS.put("loki", o -> {
            var s = new LokiSink(o.string("url"));
            o.table("labels").ifPresent(s::labels);
            o.duration("timeout").ifPresent(s::timeout);
            o.bool("connect").ifPresent(s::connect);
            return s;
        });
        SINKS.put("mail", Kinds::mail);
        SINKS.put(
                "memory",
                o -> new MemorySink(o.integer("size").map(Long::intValue).orElse(1000)));

        STEPS.put("redact", o -> {
            var r = Redact.keys(o.strings("keys").orElse(List.of()).toArray(String[]::new))
                    .paths(o.strings("paths").orElse(List.of()).toArray(String[]::new))
                    .patterns(o.strings("patterns").orElse(List.of()).toArray(String[]::new));
            return o.string("mask", null).map(r::mask).orElse(r);
        });
        STEPS.put("sanitise", _ -> new Sanitise());
        STEPS.put("fields", o -> new Fields(o.all()));
        STEPS.put("match", Kinds::match);
        STEPS.put("exclude", o -> match(o).negate());
        STEPS.put(
                "sample",
                o -> new Sample(
                        o.integer("first").orElse(100L),
                        o.integer("every").orElse(100L),
                        o.duration("per").orElse(Duration.ofSeconds(1))));
        STEPS.put("limit", o -> {
            var rate = o.number("rate").orElseThrow(() -> new SerlogException.Config("limit: `rate` is required"));
            return new Limit(rate, o.number("burst").orElse(rate));
        });
        STEPS.put("dedupe", o -> new Dedupe(o.duration("within").orElse(Duration.ofSeconds(1))));

        ALARMS.put("tiers", o -> {
            var tiers = o.get("tiers").map(Kinds::tiers).orElse(List.of(List.of("stderr")));
            return new Tiers(
                    tiers,
                    o.string("on_fail", null).map(Alarm.Fallback::parse).orElse(Alarm.Fallback.IGNORE),
                    o.duration("remind").orElse(Duration.ofHours(1)));
        });
    }

    private static List<List<String>> tiers(Object v) {
        if (!(v instanceof List<?> l)) throw new SerlogException.Config("`tiers` takes a list of lists of sink names");
        return l.stream()
                .map(t ->
                        t instanceof List<?> x ? x.stream().map(String::valueOf).toList() : List.of(String.valueOf(t)))
                .toList();
    }

    private static Sink file(Options o) {
        var s = new FileSink(o.string("path"));
        o.bool("append").ifPresent(s::append);
        o.get("perms")
                .ifPresent(
                        p -> s.perms(p instanceof Number n ? Integer.toOctalString(n.intValue()) : String.valueOf(p)));
        o.bool("mkdir").ifPresent(s::mkdir);
        o.size("max_size").ifPresent(s::maxSize);
        o.string("rotate", null).ifPresent(r -> s.rotate(FileSink.Rotate.parse(r)));
        o.integer("keep").ifPresent(k -> s.keep(k.intValue()));
        o.duration("max_age").ifPresent(s::maxAge);
        o.size("max_total").ifPresent(s::maxTotal);
        o.bool("gzip").ifPresent(s::gzip);
        o.string("roll_name", null).ifPresent(s::rollName);
        o.bool("reopen").ifPresent(s::reopen);
        var every = o.duration("flush_every").orElse(Duration.ofSeconds(1));
        o.size("buffer").ifPresent(b -> s.buffer(b, every));
        return s;
    }

    private static Sink mail(Options o) {
        var s = new MailSink(o.string("host"));
        o.strings("to").ifPresent(t -> s.to(t.toArray(String[]::new)));
        o.integer("port").ifPresent(p -> s.port(p.intValue()));
        o.string("security", null).ifPresent(v -> s.security(MailSink.Security.parse(v)));
        var user = o.string("user", null);
        var pass = o.string("pass", "password");
        if (user.isPresent()) s.login(user.get(), pass.orElse(null));
        o.string("from", "sender").ifPresent(s::from);
        o.duration("timeout").ifPresent(s::timeout);
        o.bool("connect").ifPresent(s::connect);
        return s;
    }

    private static Match match(Options o) {
        var fields = o.get("fields")
                .map(f -> {
                    if (!(f instanceof Map<?, ?> m)) throw new SerlogException.Config("match: `fields` takes a map");
                    var out = new java.util.LinkedHashMap<String, Object>();
                    m.forEach((k, v) -> out.put(String.valueOf(k), v));
                    return Match.fields(out).fields();
                })
                .orElse(Map.of());
        return new Match(
                o.strings("tags").orElse(List.of()),
                fields,
                o.get("level").map(Level::from).orElse(null),
                false);
    }

    /// Lets config files name a kind of sink.
    public static void sink(String kind, Function<Options, Sink> make) {
        SINKS.put(kind, make);
    }

    /// Lets config files name a processor in `process`.
    public static void processor(String name, Function<Options, Processor> make) {
        STEPS.put(name, make);
    }

    /// Lets config files name a layout with `layout: name`.
    public static void layout(String name, Supplier<Layout> make) {
        LAYOUTS.put(name, make);
    }

    /// Lets config files name a kind of alarm with `kind: name` under `alarms`.
    public static void alarm(String kind, Function<Options, Alarm> make) {
        ALARMS.put(kind, make);
    }

    private static <T> T find(Map<String, T> table, String name, String what) {
        var k = table.get(name);
        if (k == null) throw new SerlogException.Kind("unknown " + what + " kind `" + name + "`");
        return k;
    }

    /// The factory for a sink kind.
    public static Function<Options, Sink> sinkKind(String kind) {
        return find(SINKS, kind, "sink");
    }

    /// The factory for a processor.
    public static Function<Options, Processor> stepKind(String name) {
        return find(STEPS, name, "processor");
    }

    /// The layout with this name.
    public static Layout layout(String name) {
        return find(LAYOUTS, name, "layout").get();
    }

    /// The factory for an alarm kind.
    public static Function<Options, Alarm> alarmKind(String kind) {
        return find(ALARMS, kind, "alarm");
    }
}
