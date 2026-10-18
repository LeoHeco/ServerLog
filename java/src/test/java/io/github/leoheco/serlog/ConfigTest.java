package io.github.leoheco.serlog;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.leoheco.serlog.config.Config;
import io.github.leoheco.serlog.config.Format;
import io.github.leoheco.serlog.internal.Start;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.TimeUnit;
import java.util.stream.Stream;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.TestFactory;
import org.junit.jupiter.api.io.TempDir;
import tools.jackson.databind.json.JsonMapper;

/// Config files and start-up: every key, custom kinds, errors, export in three formats, reloading and profiles.
class ConfigTest {
    private static final String CONFIG = """
            app: demo
            level: trace
            watch: 1s
            levels:
              notice: { value: 35, colour: blue }
            process:
              - fields: { region: cn }
            redact: [pass]
            alarms:
              ops:
                tiers: [[oncall], [stderr]]
                remind: 30m
            sinks:
              - { kind: captured, label: all, name: all, format: json }
              - { kind: captured, label: notices, level: notice, pattern: "{level}:{msg}" }
              - { kind: captured, label: oncall, name: oncall, alarm_only: true }
              - { kind: file, name: app, path: "${SERLOG_TEST_DIR}/app.log", max_size: 1MiB, keep: 3, on_fail: ops, queue: 100, wait: 50ms }
            """;

    private record Ran(String out, String err, int code) {}

    private static Ran run(Path dir, String scenario, Map<String, String> env)
            throws IOException, InterruptedException {
        var cmd = new ArrayList<>(List.of(
                Path.of(System.getProperty("java.home"), "bin", "java").toString(),
                "-cp",
                System.getProperty("java.class.path"),
                Scenario.class.getName(),
                scenario));
        var pb = new ProcessBuilder(cmd).directory(dir.toFile());
        pb.environment().remove("FORCE_COLOR");
        pb.environment().put("NO_COLOR", "1");
        pb.environment().putAll(env);
        var p = pb.start();
        var out = p.getInputStream().readAllBytes();
        var err = p.getErrorStream().readAllBytes();
        if (!p.waitFor(60, TimeUnit.SECONDS)) p.destroyForcibly();
        return new Ran(new String(out, StandardCharsets.UTF_8), new String(err, StandardCharsets.UTF_8), p.exitValue());
    }

    @Test
    @SuppressWarnings("unchecked")
    void loadsDumpsAndReloads(@TempDir Path dir) throws Exception {
        var r = run(
                dir,
                "reload",
                Map.of("CONFIG", CONFIG, "SERLOG_TEST_DIR", dir.toString().replace('\\', '/')));
        assertEquals(0, r.code(), r.err());
        var lines = r.out().strip().split("\n");
        var out = (Map<String, Object>) JsonMapper.builder().build().readValue(lines[lines.length - 1], Map.class);
        var all = (List<String>) out.get("all");
        assertTrue(
                all.stream()
                        .anyMatch(l -> l.contains("\"msg\":\"hello\"")
                                && l.contains("\"region\":\"cn\"")
                                && l.contains("\"pass\":\"***\"")),
                all.toString());
        assertEquals(List.of("NOTICE:notable"), out.get("notices"));
        assertTrue(((String) out.get("file")).contains("hello"));
        assertEquals(List.of("all", "captured-1", "oncall", "app"), out.get("names"));
        var yaml = (String) out.get("yaml");
        for (var want : List.of(
                "notice", "region", "redact", "ops", "remind: 30m", "max_size: 1MiB", "on_fail: ops", "queue: 100"))
            assertTrue(yaml.contains(want), want + " in\n" + yaml);
        assertEquals(true, out.get("same"), String.valueOf(out.get("trees")));
        assertTrue(((List<String>) out.get("after")).stream().anyMatch(l -> l.endsWith("reloaded")));
        assertEquals(List.of("after"), out.get("names_after"));
    }

    @Test
    void readsTheSameTreeFromEveryFormat() {
        var y = Config.read("filter: info\nsinks:\n  - kind: console\n    stream: stderr\n", Format.YAML);
        var j = Config.read(
                "{\"filter\":\"info\",\"sinks\":[{\"kind\":\"console\",\"stream\":\"stderr\"}]}", Format.JSON);
        var t = Config.read("filter = \"info\"\n[[sinks]]\nkind = \"console\"\nstream = \"stderr\"\n", Format.TOML);
        assertEquals(y, j);
        assertEquals(j, t);
    }

    @TestFactory
    Stream<DynamicTest> reportsMistakesByName() {
        List<Map.Entry<Map<String, Object>, String>> cases = List.of(
                Map.entry(Map.of("sinks", List.of(Map.of("kind", "console", "colr", "never"))), "unknown key `colr`"),
                Map.entry(Map.of("bogus", 1), "unknown key `bogus`"),
                Map.entry(
                        Map.of("sinks", List.of(Map.of("kind", "console", "level", "info", "filter", "info"))),
                        "not both"),
                Map.entry(Map.of("sinks", List.of(Map.of("kind", "console", "on_fail", "pager"))), "not in `alarms`"),
                Map.entry(Map.of("sinks", List.of(Map.of("kind", "console", "format", "xml"))), "format"),
                Map.entry(
                        Map.of("alarms", Map.of("panic", Map.of("tiers", List.of(List.of("stderr"))))), "cannot name"),
                Map.entry(Map.of("process", List.of(Map.of("shout", Map.of()))), "shout"),
                Map.entry(
                        Map.of(
                                "alarms",
                                Map.of("ops", Map.of("tiers", List.of(List.of("nobody")))),
                                "sinks",
                                List.of(Map.of("kind", "memory", "on_fail", "ops"))),
                        "nobody"));
        return cases.stream()
                .map(c -> DynamicTest.dynamicTest(c.getValue(), () -> {
                    var e = assertThrows(SerlogException.class, () -> Start.plan(c.getKey(), List.of()));
                    assertTrue(e.getMessage().contains(c.getValue()), e.getMessage());
                }));
    }

    @Test
    void unknownKindsAndVariables() {
        assertThrows(
                SerlogException.Kind.class,
                () -> Start.plan(Map.of("sinks", List.of(Map.of("kind", "kafka"))), List.of()));
        var e = assertThrows(SerlogException.Config.class, () -> Config.expand(Map.of("x", "${SERLOG_SURELY_UNSET}")));
        assertTrue(e.getMessage().contains("SERLOG_SURELY_UNSET"));
    }

    @Test
    @SuppressWarnings("unchecked")
    void zeroConfigFollowsTheEnvironment(@TempDir Path dir) throws Exception {
        var r = run(dir, "zero", Map.of("SERLOG_FILTER", "warn", "SERLOG_FORMAT", "json"));
        assertEquals(0, r.code(), r.err());
        var rec = (Map<String, Object>)
                JsonMapper.builder().build().readValue(r.out().strip(), Map.class);
        assertEquals(List.of("warn", "loud", 1), List.of(rec.get("level"), rec.get("msg"), rec.get("n")));
    }

    @Test
    void debugAddsNamespacesAtDebug(@TempDir Path dir) throws Exception {
        var r = run(dir, "debug", Map.of("DEBUG", "app:*,-app:noisy"));
        assertEquals(0, r.code(), r.err());
        assertEquals(
                List.of("DEBUG app:db: on"),
                r.out().strip().lines().map(l -> l.substring(24)).toList());
    }

    @Test
    void profilesChangePatternStreamAndLevel(@TempDir Path dir) throws Exception {
        var r = run(dir, "python", Map.of());
        assertEquals(0, r.code(), r.err());
        assertEquals("", r.out());
        assertEquals(
                List.of("WARNING:Scenario:shown", "CRITICAL:Scenario:worse"),
                r.err().strip().lines().toList());
    }

    @Test
    void initRunsOnce(@TempDir Path dir) throws Exception {
        var r = run(dir, "twice", Map.of());
        assertNotEquals(0, r.code());
        assertTrue(r.err().contains("already initialised"), r.err());
    }

    @Test
    void anEmptyConfigFileStartsTheConsole(@TempDir Path dir) throws Exception {
        var r = run(dir, "empty", Map.of());
        assertEquals(0, r.code(), r.err());
        assertTrue(r.out().endsWith("Scenario: up\n"), r.out());
    }

    @Test
    void aBackgroundPanicExitsWith70(@TempDir Path dir) throws Exception {
        var r = run(dir, "panic", Map.of());
        assertEquals(70, r.code(), r.err());
        assertTrue(r.err().contains("gone"), r.err());
    }
}
