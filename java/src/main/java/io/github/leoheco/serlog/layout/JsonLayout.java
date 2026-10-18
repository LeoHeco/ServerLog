package io.github.leoheco.serlog.layout;

import static io.github.leoheco.serlog.layout.Text.quote;

import io.github.leoheco.serlog.Compat;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.Meta;
import io.github.leoheco.serlog.Theme;
import io.github.leoheco.serlog.internal.Hub;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.stream.Collectors;

/// JSON lines in the shape other tools expect: serlog's own, pino, bunyan, zap, slog, structlog, logstash, ECS, GELF
/// or the OpenTelemetry log model. `keys` renames any key, `levels` writes levels as names or numbers, and `nest`
/// puts the fields under one key.
///
/// @param schema the shape
/// @param keys new names for keys
/// @param levels how levels are written, or `null` for the schema's way
/// @param nest the key the fields go under, or `null` to spread them
public record JsonLayout(Schema schema, Map<String, String> keys, Levels levels, String nest) implements Layout {
    /// A JSON shape.
    public enum Schema {
        SERLOG,
        PINO,
        BUNYAN,
        ZAP,
        SLOG,
        STRUCTLOG,
        LOGSTASH,
        ECS,
        GELF,
        OTEL;

        /// A schema by its config name.
        public static Schema parse(String name) {
            try {
                return valueOf(name.strip().toUpperCase(Locale.ROOT));
            } catch (IllegalArgumentException e) {
                throw new IllegalArgumentException("unknown JSON schema `" + name + "`", e);
            }
        }

        /// The config name.
        public String id() {
            return name().toLowerCase(Locale.ROOT);
        }
    }

    /// How levels are written.
    public enum Levels {
        NAME,
        NUMBER;

        /// A style by its config name.
        public static Levels parse(String name) {
            return valueOf(name.strip().toUpperCase(Locale.ROOT));
        }

        /// The config name.
        public String id() {
            return name().toLowerCase(Locale.ROOT);
        }
    }

    private record Pair(String key, String json) {}

    private static final long PID = ProcessHandle.current().pid();

    public JsonLayout {
        keys = Map.copyOf(keys);
    }

    /// serlog's own schema.
    public JsonLayout() {
        this(Schema.SERLOG);
    }

    /// A schema with its own keys and level style.
    public JsonLayout(Schema schema) {
        this(schema, Map.of(), null, null);
    }

    private String object(List<Pair> pairs) {
        return pairs.stream()
                .map(p -> quote(keys.getOrDefault(p.key(), p.key())) + ":" + p.json())
                .collect(Collectors.joining(",", "{", "}"));
    }

    private String level(LogRecord rec, String name, boolean numeric) {
        var style = levels != null ? levels : numeric ? Levels.NUMBER : Levels.NAME;
        return style == Levels.NUMBER ? String.valueOf(rec.level().value()) : quote(name);
    }

    private void fields(LogRecord rec, String error, List<Pair> out) {
        var pairs = rec.fields().stream()
                .map(f -> new Pair(f.key().equals("error") ? error : f.key(), Text.json(f.value())))
                .toList();
        if (nest != null && !pairs.isEmpty()) out.add(new Pair(nest, object(pairs)));
        else out.addAll(pairs);
    }

    private static int logback(Level l) {
        var v = l.value();
        return v <= 10 ? 5000 : v <= 20 ? 10000 : v <= 30 ? 20000 : v <= 40 ? 30000 : 40000;
    }

    private static int otel(Level l) {
        var v = l.value();
        return v <= 10 ? 1 : v <= 20 ? 5 : v <= 30 ? 9 : v <= 40 ? 13 : v <= 50 ? 17 : 21;
    }

    private static String seconds(LogRecord rec) {
        var ms = rec.time().toEpochMilli();
        return ms / 1000 + "." + String.format("%03d", ms % 1000);
    }

    @Override
    public String render(LogRecord rec, Theme theme) {
        var m = Hub.meta();
        var lvl = rec.level();
        var out = new ArrayList<Pair>(16);
        var file = rec.file();
        var line = rec.line() > 0 ? String.valueOf(rec.line()) : null;
        switch (schema) {
            case SERLOG -> {
                add(out, "time", quote(Clock.iso(rec.time(), 3)));
                add(out, "level", level(rec, lvl.id(), false));
                add(out, "target", quote(rec.target()));
                add(out, "msg", quote(rec.msg()));
                if (file != null) add(out, "file", quote(file));
                if (line != null) add(out, "line", line);
                fields(rec, "error", out);
                ids(m, out);
            }
            case PINO -> {
                add(out, "level", level(rec, lvl.id(), true));
                add(out, "time", String.valueOf(rec.time().toEpochMilli()));
                add(out, "pid", String.valueOf(PID));
                add(out, "hostname", quote(Meta.host()));
                fields(rec, "err", out);
                add(out, "msg", quote(rec.msg()));
            }
            case BUNYAN -> {
                add(out, "name", quote(m.app()));
                add(out, "hostname", quote(Meta.host()));
                add(out, "pid", String.valueOf(PID));
                add(out, "level", level(rec, lvl.id(), true));
                fields(rec, "err", out);
                add(out, "msg", quote(rec.msg()));
                add(out, "time", quote(Clock.iso(rec.time(), 3)));
                add(out, "v", "0");
            }
            case ZAP -> {
                add(out, "level", level(rec, lvl.id(), false));
                add(out, "ts", seconds(rec));
                add(out, "logger", quote(rec.target()));
                if (file != null) add(out, "caller", quote(line != null ? file + ":" + line : file));
                add(out, "msg", quote(rec.msg()));
                fields(rec, "error", out);
            }
            case SLOG -> {
                add(out, "time", quote(Clock.strftime(rec.time(), "%Y-%m-%dT%H:%M:%S%.9f%:z")));
                add(out, "level", level(rec, lvl.label(Compat.SLOG), false));
                add(out, "msg", quote(rec.msg()));
                fields(rec, "error", out);
            }
            case STRUCTLOG -> {
                add(out, "event", quote(rec.msg()));
                fields(rec, "error", out);
                add(out, "level", level(rec, lvl.id(), false));
                add(out, "logger", quote(rec.target()));
                add(out, "timestamp", quote(Clock.iso(rec.time(), 3)));
            }
            case LOGSTASH -> {
                add(out, "@timestamp", quote(Clock.strftime(rec.time(), "%Y-%m-%dT%H:%M:%S%.3f%:z")));
                add(out, "@version", quote("1"));
                add(out, "message", quote(rec.msg()));
                add(out, "logger_name", quote(rec.target()));
                if (rec.thread() != null) add(out, "thread_name", quote(rec.thread()));
                add(out, "level", level(rec, lvl.label(Compat.LOGBACK), false));
                add(out, "level_value", String.valueOf(logback(lvl)));
                fields(rec, "stack_trace", out);
            }
            case ECS -> {
                add(out, "@timestamp", quote(Clock.iso(rec.time(), 3)));
                add(out, "log.level", level(rec, lvl.id(), false));
                add(out, "message", quote(rec.msg()));
                add(out, "ecs.version", quote("8.11.0"));
                add(out, "log.logger", quote(rec.target()));
                if (file != null) add(out, "log.origin.file.name", quote(file));
                if (line != null) add(out, "log.origin.file.line", line);
                add(out, "process.pid", String.valueOf(PID));
                add(out, "host.hostname", quote(Meta.host()));
                if (!m.app().isEmpty()) add(out, "service.name", quote(m.app()));
                if (!m.env().isEmpty()) add(out, "service.environment", quote(m.env()));
                if (!m.instance().isEmpty()) add(out, "service.node.name", quote(m.instance()));
                fields(rec, "error", out);
            }
            case GELF -> {
                add(out, "version", quote("1.1"));
                add(out, "host", quote(Meta.host()));
                add(out, "short_message", quote(rec.msg()));
                add(out, "timestamp", seconds(rec));
                add(out, "level", String.valueOf(lvl.syslog()));
                add(out, "_target", quote(rec.target()));
                for (var f : rec.fields()) if (!f.key().equals("id")) add(out, "_" + f.key(), Text.json(f.value()));
            }
            case OTEL -> {
                var attrs = new ArrayList<Pair>();
                add(attrs, "code.namespace", quote(rec.target()));
                if (file != null) add(attrs, "code.filepath", quote(file));
                if (line != null) add(attrs, "code.lineno", line);
                for (var f : rec.fields())
                    add(attrs, f.key().equals("error") ? "exception" : f.key(), Text.json(f.value()));
                var res = new ArrayList<Pair>();
                if (!m.app().isEmpty()) add(res, "service.name", quote(m.app()));
                if (!m.env().isEmpty()) add(res, "deployment.environment", quote(m.env()));
                if (!m.instance().isEmpty()) add(res, "service.instance.id", quote(m.instance()));
                add(out, "timestamp", quote(Clock.iso(rec.time(), 9)));
                add(out, "severity_text", quote(lvl.label(Compat.SERLOG)));
                add(out, "severity_number", String.valueOf(otel(lvl)));
                add(out, "body", quote(rec.msg()));
                add(out, "attributes", object(attrs));
                add(out, "resource", object(res));
            }
        }
        return object(out);
    }

    private static void add(List<Pair> out, String key, String json) {
        out.add(new Pair(key, json));
    }

    private static void ids(Meta m, List<Pair> out) {
        if (!m.app().isEmpty()) add(out, "app", quote(m.app()));
        if (!m.env().isEmpty()) add(out, "env", quote(m.env()));
        if (!m.instance().isEmpty()) add(out, "instance", quote(m.instance()));
    }
}
