package io.github.leoheco.serlog;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.sun.net.httpserver.HttpServer;
import io.github.leoheco.serlog.layout.PatternLayout;
import io.github.leoheco.serlog.sink.FileSink;
import io.github.leoheco.serlog.sink.LokiSink;
import io.github.leoheco.serlog.sink.MailSink;
import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.PrintWriter;
import java.net.InetSocketAddress;
import java.net.ServerSocket;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.zip.GZIPInputStream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.DisabledOnOs;
import org.junit.jupiter.api.condition.OS;
import org.junit.jupiter.api.io.TempDir;
import tools.jackson.databind.json.JsonMapper;

/// The reference sinks, checked against the file system and stand-in Loki and SMTP servers.
class SinksTest {
    private static final PatternLayout MSG = new PatternLayout("{msg}");

    private static void put(FileSink f, String msg, Instant at, Level level) throws IOException {
        f.emit(new Entry(LogRecord.of(level, "t", msg).withTime(at), MSG));
    }

    private static void put(FileSink f, String msg) throws IOException {
        put(f, msg, Instant.now(), Level.INFO);
    }

    private static List<String> names(Path d) throws IOException {
        try (var s = Files.list(d)) {
            return s.map(p -> p.getFileName().toString()).sorted().toList();
        }
    }

    private static String text(Path p) throws IOException {
        if (!p.toString().endsWith(".gz")) return Files.readString(p);
        try (var in = new GZIPInputStream(Files.newInputStream(p))) {
            return new String(in.readAllBytes(), StandardCharsets.UTF_8);
        }
    }

    @Test
    void rollsBySizeKeepsTheNewestAndGzips(@TempDir Path d) throws Exception {
        var f = new FileSink(d.resolve("app.log")).maxSize(150).keep(2).gzip(true);
        f.ready();
        var start = Instant.now();
        for (var i = 0; i < 12; i++) {
            put(f, "line %02d %s".formatted(i, "x".repeat(60)), start.plusSeconds(i), Level.INFO);
            f.flush();
        }
        var rolled = names(d).stream().filter(n -> n.endsWith(".log.gz")).toList();
        assertEquals(2, rolled.size());
        assertEquals(3, names(d).size());
        assertTrue(text(d.resolve("app.log")).contains("line 11"));
        var joined = new StringBuilder();
        for (var n : rolled) joined.append(text(d.resolve(n)));
        assertTrue(joined.toString().contains("line 07") && joined.toString().contains("line 09"));
        assertFalse(joined.toString().contains("line 05"));
    }

    @Test
    void rotatesWhenThePeriodTurns(@TempDir Path d) throws Exception {
        var f = new FileSink(d.resolve("day.log")).rotate(FileSink.Rotate.DAY);
        f.ready();
        var now = Instant.now();
        put(f, "today", now, Level.INFO);
        put(f, "still today", now, Level.INFO);
        put(f, "tomorrow", now.plus(Duration.ofHours(25)), Level.INFO);
        f.flush();
        var old = names(d).stream().filter(n -> !n.equals("day.log")).toList();
        assertEquals(1, old.size());
        assertTrue(text(d.resolve(old.getFirst())).contains("still today"));
        assertEquals("tomorrow\n", text(d.resolve("day.log")));
    }

    @Test
    void namesRolledFilesFromATemplate(@TempDir Path d) throws Exception {
        var f = new FileSink(d.resolve("svc.log")).maxSize(10).rollName("{stem}-old.{ext}");
        f.ready();
        for (var m : List.of("first line", "second line", "third line")) put(f, m);
        f.flush();
        assertEquals(List.of("svc-old-1.log", "svc-old.log", "svc.log"), names(d));
    }

    @Test
    void cleansUpByTotalSizeAndLeavesOthers(@TempDir Path d) throws Exception {
        Files.writeString(d.resolve("big.yaml"), "x");
        var f = new FileSink(d.resolve("big.log")).maxSize(100).maxTotal(250);
        f.ready();
        for (var i = 0; i < 8; i++) {
            put(f, i + " " + "y".repeat(97));
            f.flush();
        }
        var total = 0L;
        for (var n : names(d)) if (!n.equals("big.log") && !n.equals("big.yaml")) total += Files.size(d.resolve(n));
        assertTrue(total <= 250);
        assertTrue(Files.exists(d.resolve("big.yaml")));
    }

    @Test
    @DisabledOnOs(value = OS.WINDOWS, disabledReason = "Windows does not let others move a file Java holds open")
    void reopensAfterTheFileIsMovedAway(@TempDir Path d) throws Exception {
        var path = d.resolve("moved.log");
        var f = new FileSink(path).reopen(true);
        f.ready();
        put(f, "before");
        f.flush();
        Files.move(path, d.resolve("moved.log.1"));
        Thread.sleep(1100);
        put(f, "after");
        f.flush();
        assertEquals("after\n", text(path));
    }

    @Test
    void truncatesAndMakesDirectories(@TempDir Path d) throws Exception {
        var path = d.resolve("t.log");
        Files.writeString(path, "old\n");
        var f = new FileSink(path).append(false);
        f.ready();
        put(f, "new");
        assertEquals("new\n", text(path));
        new FileSink(d.resolve("a/b/c.log")).ready();
        assertTrue(Files.exists(d.resolve("a/b/c.log")));
        assertThrows(
                SerlogException.Config.class,
                () -> new FileSink(d.resolve("x/y.log")).mkdir(false).ready());
    }

    @Test
    void buffersUntilAnError(@TempDir Path d) throws Exception {
        var path = d.resolve("buf.log");
        var f = new FileSink(path).buffer(64 << 10, Duration.ofHours(1));
        f.ready();
        put(f, "first");
        put(f, "second");
        assertEquals("first\n", text(path));
        put(f, "bad", Instant.now(), Level.ERROR);
        assertEquals("first\nsecond\nbad\n", text(path));
    }

    @Test
    void aFileClaimsItsCanonicalPath(@TempDir Path d) throws Exception {
        Files.createDirectories(d.resolve("x"));
        assertEquals(new FileSink(d.resolve("x/../same.log")).claim(), new FileSink(d.resolve("same.log")).claim());
    }

    private record Seen(String method, String path, Map<String, List<String>> headers, String body) {}

    private static HttpServer stand(List<Integer> answers, List<Seen> seen) throws IOException {
        var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/", x -> {
            var raw = x.getRequestBody().readAllBytes();
            var body = raw.length == 0
                    ? ""
                    : new String(
                            new GZIPInputStream(new java.io.ByteArrayInputStream(raw)).readAllBytes(),
                            StandardCharsets.UTF_8);
            seen.add(new Seen(x.getRequestMethod(), x.getRequestURI().getPath(), x.getRequestHeaders(), body));
            x.sendResponseHeaders(answers.isEmpty() ? 204 : answers.removeFirst(), -1);
            x.close();
        });
        server.start();
        return server;
    }

    private static String url(HttpServer s) {
        return "http://127.0.0.1:" + s.getAddress().getPort() + "/loki/api/v1/push";
    }

    @Test
    @SuppressWarnings("unchecked")
    void lokiPushesOneStreamPerLevel() throws Exception {
        var seen = Collections.synchronizedList(new ArrayList<Seen>());
        var server = stand(new ArrayList<>(), seen);
        try {
            var loki = new LokiSink(url(server)).labels(Map.of("team", "mirrors"));
            var lay = loki.defaultLayout().orElseThrow();
            loki.emitBatch(List.of(
                    new Entry(
                            LogRecord.of(
                                            Level.INFO,
                                            "sync",
                                            "started",
                                            List.of(Field.of("mirror", "debian"), Field.of("tries", 3)))
                                    .withCaller(null, null, "A.java", 3),
                            lay),
                    new Entry(LogRecord.of(Level.ERROR, "sync", "failed \"rsync\""), lay),
                    new Entry(LogRecord.of(Level.INFO, "sync", "retrying"), lay)));
            var got = seen.getFirst();
            assertEquals("/loki/api/v1/push", got.path());
            assertEquals(List.of("gzip"), got.headers().get("Content-encoding"));
            assertEquals(List.of("application/json"), got.headers().get("Content-type"));
            var streams = (List<Map<String, Object>>) JsonMapper.builder()
                    .build()
                    .readValue(got.body(), Map.class)
                    .get("streams");
            var info = streams.get(0);
            var err = streams.get(1);
            var labels = (Map<String, String>) info.get("stream");
            assertEquals(List.of("info", "mirrors"), List.of(labels.get("level"), labels.get("team")));
            var values = (List<List<Object>>) info.get("values");
            assertEquals(
                    List.of("started", "retrying"),
                    values.stream().map(v -> v.get(1)).toList());
            assertEquals(
                    Map.of("target", "sync", "file", "A.java:3", "mirror", "debian", "tries", "3"),
                    values.getFirst().get(2));
            assertEquals(
                    "failed \"rsync\"",
                    ((List<List<Object>>) err.get("values")).getFirst().get(1));
        } finally {
            server.stop(0);
        }
    }

    @Test
    void lokiTellsRetryableAnswersFromRefusals() throws Exception {
        var server = stand(new ArrayList<>(List.of(503, 400)), new ArrayList<>());
        try {
            var loki = new LokiSink(url(server));
            var e = new Entry(
                    LogRecord.of(Level.INFO, "t", "x"), loki.defaultLayout().orElseThrow());
            var first = assertThrows(IOException.class, () -> loki.emit(e));
            assertTrue(first.getMessage().contains("503"));
            assertThrows(SerlogException.Rejected.class, () -> loki.emit(e));
        } finally {
            server.stop(0);
        }
    }

    @Test
    void lokiChecksItsUrlAndAsksReady() throws Exception {
        var seen = Collections.synchronizedList(new ArrayList<Seen>());
        var server = stand(new ArrayList<>(), seen);
        try {
            assertThrows(SerlogException.Config.class, () -> new LokiSink("ftp://x").ready());
            new LokiSink(url(server)).connect(true).ready();
            assertEquals(
                    List.of("GET", "/ready"),
                    List.of(seen.getLast().method(), seen.getLast().path()));
        } finally {
            server.stop(0);
        }
    }

    private static Thread smtp(ServerSocket server, ConcurrentLinkedQueue<String> data) {
        return Thread.ofVirtual().start(() -> {
            while (!server.isClosed()) {
                try (var sock = server.accept();
                        var in = new BufferedReader(
                                new InputStreamReader(sock.getInputStream(), StandardCharsets.UTF_8));
                        var out = new PrintWriter(sock.getOutputStream(), true, StandardCharsets.UTF_8)) {
                    out.print("220 test\r\n");
                    out.flush();
                    for (var line = in.readLine(); line != null; line = in.readLine()) {
                        var cmd = line.toUpperCase();
                        if (cmd.startsWith("EHLO")) out.print("250-test\r\n250 OK\r\n");
                        else if (cmd.startsWith("DATA")) {
                            out.print("354 go on\r\n");
                            out.flush();
                            var body = new StringBuilder();
                            for (var part = in.readLine(); part != null && !part.equals("."); part = in.readLine())
                                body.append(part).append('\n');
                            data.add(body.toString());
                            out.print("250 queued\r\n");
                        } else if (cmd.startsWith("QUIT")) {
                            out.print("221 bye\r\n");
                            out.flush();
                            break;
                        } else out.print("250 OK\r\n");
                        out.flush();
                    }
                } catch (IOException _) {
                }
            }
        });
    }

    @Test
    void mailSendsOneMessagePerBatch() throws Exception {
        var data = new ConcurrentLinkedQueue<String>();
        try (var server = new ServerSocket(0)) {
            smtp(server, data);
            var m = new MailSink("127.0.0.1")
                    .port(server.getLocalPort())
                    .security(MailSink.Security.NONE)
                    .from("bot@example.org")
                    .to("ops@example.org");
            m.ready();
            assertFalse(m.enabled(Level.WARN, "x"));
            assertTrue(m.enabled(Level.ERROR, "x"));
            var lay = new PatternLayout("{level} {msg}");
            m.emitBatch(List.of(
                    new Entry(LogRecord.of(Level.ERROR, "sync", "disk <full>"), lay),
                    new Entry(LogRecord.of(Level.FATAL, "sync", "mirror stalled"), lay)));
            var all = String.join("", data);
            assertTrue(all.contains("2 FATAL records"), all);
            assertTrue(all.contains("ERROR disk <full>"));
            assertTrue(all.contains("disk &lt;full&gt;"));
            assertTrue(all.contains("mirror stalled"));
        }
    }

    @Test
    void mailRefusesToStartWithoutAddresses() {
        for (var m : List.of(
                new MailSink("h").to("ops@example.org"),
                new MailSink("h").from("bot@example.org"),
                new MailSink("h").login("bot@example.org", null).to("ops@example.org")))
            assertThrows(SerlogException.Config.class, m::ready);
    }

    @Test
    void theMailTemplateMatchesTheSpec() throws IOException {
        assertEquals(Files.readString(Vectors.SPEC.resolve("mail.html")), MailSink.page());
    }
}
