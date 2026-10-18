package io.github.leoheco.serlog;

import io.github.leoheco.serlog.config.Config;
import io.github.leoheco.serlog.config.Format;
import io.github.leoheco.serlog.config.Kinds;
import io.github.leoheco.serlog.sink.MemorySink;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.ConcurrentHashMap;
import tools.jackson.databind.json.JsonMapper;

/// Programs the config tests run in a fresh JVM, one per name.
public final class Scenario {
    private static final Map<String, MemorySink> CAPTURED = new ConcurrentHashMap<>();

    static final class Captured extends MemorySink {
        final String label;

        Captured(String label) {
            super(100);
            this.label = label;
            CAPTURED.put(label, this);
        }

        @Override
        public String kind() {
            return "captured";
        }

        @Override
        public Optional<Map<String, Object>> options() {
            return Optional.of(Map.of("label", label));
        }
    }

    private Scenario() {}

    public static void main(String[] args) throws Exception {
        switch (args[0]) {
            case "reload" -> reload();
            case "zero" -> {
                Serlog.init();
                Serlog.debug("quiet");
                Serlog.at(Level.WARN).field("n", 1).log("loud");
            }
            case "debug" -> {
                Serlog.init();
                Serlog.get("app:db").debug("on");
                Serlog.get("app:noisy").debug("off");
                Serlog.get("web").debug("no");
            }
            case "python" -> {
                Serlog.builder().compat(Compat.PYTHON).init();
                Serlog.info("hidden");
                Serlog.warn("shown");
                Serlog.fatal("worse");
            }
            case "twice" -> {
                Serlog.init();
                Serlog.init();
            }
            case "empty" -> {
                Files.writeString(Path.of("serlog.yaml"), "");
                Serlog.init();
                Serlog.info("up");
            }
            case "panic" -> {
                Serlog.builder()
                        .sink(Slot.of(_ -> {
                                    throw new java.io.IOException("gone");
                                })
                                .queue(10)
                                .wait(Duration.ofMillis(10))
                                .onFail("panic")
                                .retries(0))
                        .init();
                Serlog.info("x");
                Thread.sleep(5000);
            }
            default -> throw new IllegalArgumentException(args[0]);
        }
    }

    private static void reload() throws Exception {
        Kinds.sink("captured", o -> new Captured(o.string("label")));
        var path = Path.of("serlog.yaml");
        Files.writeString(path, System.getenv("CONFIG"));
        Config.load(path);
        Serlog.get("main").atInfo().field("pass", "secret").log("hello");
        Serlog.log(Level.of(35), "notable");
        Serlog.flush();
        var out = new LinkedHashMap<String, Object>();
        out.put("all", CAPTURED.get("all").lines());
        out.put("notices", CAPTURED.get("notices").lines());
        out.put("names", Serlog.status().stream().map(Status::name).toList());
        out.put("file", Files.readString(Path.of(System.getenv("SERLOG_TEST_DIR"), "app.log")));
        out.put("yaml", Config.dump(Format.YAML));
        var trees = List.of(Format.YAML, Format.JSON, Format.TOML).stream()
                .map(f -> Config.read(Config.dump(f), f))
                .toList();
        out.put("same", trees.get(0).equals(trees.get(1)) && trees.get(1).equals(trees.get(2)));
        if (!(Boolean) out.get("same")) out.put("trees", trees);
        Thread.sleep(1100);
        Files.writeString(path, "level: info\nsinks:\n  - { kind: captured, label: after, name: after }\n");
        Thread.sleep(2500);
        Serlog.info("reloaded");
        Serlog.flush();
        out.put("after", CAPTURED.get("after").lines());
        out.put("names_after", Serlog.status().stream().map(Status::name).toList());
        System.out.println(JsonMapper.builder().build().writeValueAsString(out));
    }
}
