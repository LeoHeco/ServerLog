package io.github.leoheco.serlog;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.leoheco.serlog.alarm.Incident;
import io.github.leoheco.serlog.alarm.Tiers;
import io.github.leoheco.serlog.internal.Breaker;
import io.github.leoheco.serlog.internal.Hub;
import io.github.leoheco.serlog.internal.Walk;
import io.github.leoheco.serlog.layout.JsonLayout;
import io.github.leoheco.serlog.layout.Layouts;
import io.github.leoheco.serlog.layout.PatternLayout;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.stream.Stream;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.TestFactory;
import tools.jackson.databind.json.JsonMapper;

/// The shared vectors in `spec/cases`, which every serlog package must pass.
@SuppressWarnings("unchecked")
class SpecTest {
    private static final JsonMapper JSON = JsonMapper.builder().build();

    @BeforeAll
    static void identity() throws Exception {
        var m = (Map<String, String>) Vectors.load("render").get("meta");
        Hub.setMeta(new Meta(m.get("app"), m.get("env"), m.get("instance")));
    }

    @AfterAll
    static void reset() {
        Hub.setMeta(Meta.detect());
    }

    private static Object value(Object v) {
        return switch (v) {
            case Map<?, ?> m
            when m.containsKey("$error") -> {
                var msgs = (List<String>) m.get("$error");
                Throwable cause = null;
                for (var msg : msgs.reversed()) cause = new Exception(msg, cause, false, false) {};
                yield cause;
            }
            case Map<?, ?> m -> {
                var out = new java.util.LinkedHashMap<Object, Object>();
                m.forEach((k, x) -> out.put(k, value(x)));
                yield out;
            }
            case List<?> l -> l.stream().map(SpecTest::value).toList();
            case null -> null;
            default -> v;
        };
    }

    private static LogRecord record(Map<String, Object> r) {
        var fields = ((List<List<Object>>) r.get("fields"))
                .stream().map(f -> Field.of((String) f.get(0), value(f.get(1)))).toList();
        var rec = LogRecord.of(
                        Level.parse((String) r.get("level")), (String) r.get("target"), (String) r.get("msg"), fields)
                .withTime(Instant.ofEpochSecond(((Number) r.get("time")).longValue()));
        return rec.withCaller(null, null, (String) r.get("file"), r.get("line") instanceof Number n ? n.intValue() : 0);
    }

    @TestFactory
    Stream<DynamicTest> renders() throws Exception {
        var cases = (List<Map<String, Object>>) Vectors.load("render").get("cases");
        return cases.stream()
                .map(c -> DynamicTest.dynamicTest(((Map<String, String>) c.get("record")).get("msg"), () -> {
                    var rec = record((Map<String, Object>) c.get("record"));
                    for (var p : (List<Map<String, String>>) c.get("patterns"))
                        assertEquals(
                                p.get("expect"),
                                new PatternLayout(p.get("pattern")).render(rec, null),
                                p.get("pattern"));
                    for (var p : (List<Map<String, String>>) c.get("presets")) {
                        var got = new PatternLayout(p.get("name")).render(rec, null);
                        if (p.containsKey("expect")) assertEquals(p.get("expect"), got, p.get("name"));
                        else assertTrue(got.endsWith(p.get("tail")), p.get("name") + ": " + got);
                    }
                    if (c.containsKey("logfmt")) assertEquals(c.get("logfmt"), Layouts.LOGFMT.render(rec, null));
                    if (c.containsKey("pretty")) {
                        var got = Layouts.PRETTY.render(rec, null);
                        assertTrue(got.endsWith((String) c.get("pretty")), got);
                    }
                    assertEquals(c.get("json"), new JsonLayout().render(rec, null));
                    for (var s : (List<Map<String, Object>>) c.get("schemas")) {
                        var schema = (String) s.get("schema");
                        var got = (Map<String, Object>) JSON.readValue(
                                new JsonLayout(JsonLayout.Schema.parse(schema)).render(rec, null), Map.class);
                        assertEquals(s.get("keys"), new ArrayList<>(got.keySet()), schema);
                        ((Map<String, Object>) s.get("has"))
                                .forEach((k, v) -> assertEquals(v, got.get(k), schema + "." + k));
                    }
                }));
    }

    @TestFactory
    Stream<DynamicTest> filters() throws Exception {
        var cases = (List<Map<String, Object>>) Vectors.load("render").get("filters");
        return cases.stream()
                .map(f -> DynamicTest.dynamicTest("filter " + f.get("spec"), () -> {
                    var filter = new Filter((String) f.get("spec"));
                    var messages = (Map<String, List<String>>) f.getOrDefault("messages", Map.of());
                    for (var key : List.of("allow", "deny")) {
                        var want = key.equals("allow");
                        for (var pair : (List<List<Object>>) f.get(key))
                            assertEquals(
                                    want,
                                    filter.allows(Level.from(pair.get(0)), (String) pair.get(1)),
                                    f.get("spec") + " " + pair);
                        for (var msg : messages.getOrDefault(key, List.of()))
                            assertEquals(want, filter.admits(msg), f.get("spec") + " " + msg);
                    }
                }));
    }

    @TestFactory
    Stream<DynamicTest> health() throws Exception {
        var cases = (List<Map<String, Object>>) Vectors.load("fail").get("cases");
        return cases.stream()
                .map(c -> DynamicTest.dynamicTest((String) c.get("name"), () -> {
                    var b = new Breaker(((Number) c.get("probe")).doubleValue(), 0);
                    var tell = Breaker.Tell.valueOf(((String) c.get("tell")).toUpperCase());
                    var remind = ((Number) c.get("remind")).doubleValue();
                    var i = 0;
                    for (var step : (List<Map<String, Object>>) c.get("steps")) {
                        var at = ((Number) step.get("at")).doubleValue();
                        var tried = b.admit(at);
                        var where = c.get("name") + " #" + i++;
                        assertEquals(step.get("tried"), tried, where);
                        Incident.Phase phase = null;
                        if (tried)
                            phase = "ok".equals(step.get("result")) ? b.passed() : b.failed(at, "boom", remind, tell);
                        assertEquals(step.get("health"), b.health().id(), where);
                        assertEquals(step.get("phase"), phase == null ? null : phase.id(), where);
                    }
                    assertEquals(((Number) c.get("skipped")).longValue(), b.skipped());
                }));
    }

    private static List<Set<String>> byTier(Map<String, Object> c, List<String> tried) {
        var left = new ArrayList<>(tried);
        var logs = (List<String>) c.get("logs");
        var out = new ArrayList<Set<String>>();
        for (var tier : (List<List<String>>) c.get("tiers")) {
            var members = tier.stream()
                    .flatMap(n -> n.equals("*") ? logs.stream() : Stream.of(n))
                    .toList();
            var here = new LinkedHashSet<String>();
            for (var n : left) if (members.contains(n)) here.add(n);
            left.removeAll(here);
            if (!here.isEmpty()) out.add(here);
        }
        return out;
    }

    @TestFactory
    Stream<DynamicTest> alarms() throws Exception {
        var cases = (List<Map<String, Object>>) Vectors.load("alarm").get("cases");
        return cases.stream()
                .map(c -> DynamicTest.dynamicTest((String) c.get("name"), () -> {
                    var tried = Collections.synchronizedList(new ArrayList<String>());
                    var results = (Map<String, String>) c.get("results");
                    var desk = new Walk.Desk() {
                        @Override
                        public List<String> logs() {
                            return (List<String>) c.get("logs");
                        }

                        @Override
                        public boolean resting(String name) {
                            return ((List<String>) c.get("down")).contains(name);
                        }

                        @Override
                        public void deliver(String name, LogRecord rec) throws Exception {
                            tried.add(name);
                            if (!"ok".equals(results.get(name))) throw new Exception("refused");
                        }
                    };
                    var incident = Incident.of((String) c.get("failing"), "custom", Incident.Phase.OPENED, "boom");
                    var heard = Walk.sound(new Tiers((List<List<String>>) c.get("tiers")), incident, desk);
                    assertEquals(c.get("outcome"), heard ? "heard" : "unheard");
                    var want = ((List<List<String>>) c.get("tried"))
                            .stream()
                                    .map(t -> (Set<String>) new LinkedHashSet<>(t))
                                    .toList();
                    assertEquals(want, byTier(c, tried));
                    assertEquals(
                            c.get("failures"),
                            incident.failures().stream()
                                    .map(Incident.Failure::sink)
                                    .toList());
                }));
    }
}
