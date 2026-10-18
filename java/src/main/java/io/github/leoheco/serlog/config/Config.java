package io.github.leoheco.serlog.config;

import io.github.leoheco.serlog.Field;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.Slot;
import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.internal.Hub;
import io.github.leoheco.serlog.internal.Start;
import io.github.leoheco.serlog.process.Processor;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/// Start-up from YAML, JSON or TOML, export of the settings in force, and reloading when the file changes. The keys
/// are those of [io.github.leoheco.serlog.Builder], as config files spell them. Needs Jackson 3 for the format used.
public final class Config {
    private static final Pattern VAR = Pattern.compile("\\$\\{([^}]*)}");

    private Config() {}

    /// Config text as a tree, without expanding variables.
    public static Map<String, Object> read(String text, Format format) {
        return Codec.read(text, format);
    }

    /// `${NAME}` in every string replaced by that environment variable; an unset one is an error.
    @SuppressWarnings("unchecked")
    public static <T> T expand(T tree) {
        return (T)
                switch (tree) {
                    case String s ->
                        VAR.matcher(s).replaceAll(m -> {
                            var v = System.getenv(m.group(1));
                            if (v == null)
                                throw new SerlogException.Config("environment variable " + m.group(1) + " is not set");
                            return Matcher.quoteReplacement(v);
                        });
                    case List<?> l -> l.stream().map(Config::expand).toList();
                    case Map<?, ?> m -> {
                        var out = new LinkedHashMap<Object, Object>();
                        m.forEach((k, v) -> out.put(k, expand(v)));
                        yield out;
                    }
                    case null -> null;
                    default -> tree;
                };
    }

    /// The sink a config entry describes, with its slot settings.
    public static Slot slot(Map<String, ?> entry) {
        var kind = entry.get("kind");
        if (!(kind instanceof String k)) throw new SerlogException.Config("each sink needs a `kind`");
        var own = new LinkedHashMap<String, Object>();
        var slotKeys = new LinkedHashMap<String, Object>();
        entry.forEach((key, v) -> {
            if (key.equals("kind")) return;
            (Slot.KEYS.contains(key) ? slotKeys : own).put(key, v);
        });
        var make = Kinds.sinkKind(k);
        var opts = new Options(k + " sink", own);
        try {
            var sink = make.apply(opts);
            opts.done();
            return Slot.of(sink).with(slotKeys);
        } catch (SerlogException e) {
            throw e;
        } catch (RuntimeException e) {
            throw new SerlogException.Config(k + " sink: " + SerlogException.message(e), e);
        }
    }

    /// The processor a `process` entry describes: a name, or a map from a name to its options.
    public static Processor step(Object spec) {
        String name;
        Object value;
        switch (spec) {
            case String s -> {
                name = s;
                value = Map.of();
            }
            case Map<?, ?> m
            when m.size() == 1 -> {
                var e = m.entrySet().iterator().next();
                name = String.valueOf(e.getKey());
                value = e.getValue() == null || Boolean.TRUE.equals(e.getValue()) ? Map.of() : e.getValue();
            }
            default -> throw new SerlogException.Config("a processor is a name or a one-key map, not " + spec);
        }
        if (!(value instanceof Map<?, ?> m)) throw new SerlogException.Config("processor `" + name + "` takes a map");
        var make = Kinds.stepKind(name);
        var keys = new LinkedHashMap<String, Object>();
        m.forEach((k, v) -> keys.put(String.valueOf(k), v));
        var opts = new Options("processor `" + name + "`", keys);
        try {
            var p = make.apply(opts);
            opts.done();
            return p;
        } catch (SerlogException e) {
            throw e;
        } catch (RuntimeException e) {
            throw new SerlogException.Config("processor `" + name + "`: " + SerlogException.message(e), e);
        }
    }

    /// The alarm an `alarms` entry describes.
    public static Alarm alarm(String name, Map<String, ?> spec) {
        if (name.equals("panic") || name.equals("ignore"))
            throw new SerlogException.Config("`" + name + "` cannot name an alarm");
        var keys = new LinkedHashMap<String, Object>(spec);
        var kind = String.valueOf(keys.getOrDefault("kind", "tiers"));
        keys.remove("kind");
        var make = Kinds.alarmKind(kind);
        var opts = new Options("alarm `" + name + "`", keys);
        try {
            var a = make.apply(opts);
            opts.done();
            return a;
        } catch (SerlogException e) {
            throw e;
        } catch (RuntimeException e) {
            throw new SerlogException.Config("alarm `" + name + "`: " + SerlogException.message(e), e);
        }
    }

    /// Starts from config text.
    public static void loads(String text, Format format) {
        Start.start(expand(read(text, format)));
    }

    /// Starts from a file, the format from its extension; reloads it if it sets `watch`.
    public static void load(Path path) {
        var tree = expand(read(text(path), Format.of(path)));
        Start.start(tree);
        var every = tree.get("watch");
        if (every != null) watch(path, io.github.leoheco.serlog.internal.Units.duration(every));
    }

    private static String text(Path path) {
        try {
            return Files.readString(path);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    /// Reloads `path` whenever its modification time changes, if all of it passes; a failure goes to `stderr`.
    public static void watch(Path path, Duration every) {
        Thread.ofVirtual().name("serlog-watch").start(() -> {
            var seen = stamp(path);
            for (; ; ) {
                try {
                    Thread.sleep(every);
                } catch (InterruptedException e) {
                    return;
                }
                var now = stamp(path);
                if (now == seen) continue;
                seen = now;
                try {
                    Start.apply(Start.plan(expand(read(text(path), Format.of(path))), List.of()), true);
                } catch (RuntimeException e) {
                    var rec = LogRecord.of(
                            Level.ERROR,
                            "serlog",
                            "reloading " + path + " failed: " + SerlogException.message(e),
                            List.of(Field.of("error", e)));
                    try {
                        Hub.BOARD.deliver("stderr", rec);
                    } catch (Exception x) {
                        System.err.println(rec.msg());
                    }
                }
            }
        });
    }

    private static long stamp(Path path) {
        try {
            return Files.getLastModifiedTime(path).toMillis();
        } catch (IOException e) {
            return -1;
        }
    }

    /// The settings in force as config text that loads back to the same behaviour; passwords come out as `***`.
    public static String dump(Format format) {
        return Codec.write(io.github.leoheco.serlog.internal.Export.tree(), format);
    }
}
