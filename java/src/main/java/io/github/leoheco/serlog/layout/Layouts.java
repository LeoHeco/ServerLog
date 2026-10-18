package io.github.leoheco.serlog.layout;

import io.github.leoheco.serlog.Compat;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.Theme;
import io.github.leoheco.serlog.config.Kinds;
import io.github.leoheco.serlog.internal.Hub;
import java.util.EnumMap;
import java.util.Map;
import java.util.function.Function;

/// The built-in layouts, and the layout a slot's keys ask for.
public final class Layouts {
    private static final Map<Compat, PatternLayout> PROFILES = new EnumMap<>(Compat.class);

    private Layouts() {}

    /// The pattern of the compat profile in force.
    public static final Layout INHERIT = new Layout() {
        @Override
        public String render(LogRecord rec, Theme theme) {
            return profile(Hub.compat()).render(rec, theme);
        }

        @Override
        public boolean caller() {
            return profile(Hub.compat()).caller();
        }
    };

    /// `time=… level=… target=… msg=… key=value…`
    public static final Layout LOGFMT = new Layout() {
        @Override
        public String render(LogRecord rec, Theme theme) {
            return Text.logfmtLine(rec);
        }

        @Override
        public boolean caller() {
            return false;
        }
    };

    /// A first line plus one line per field, for reading in a terminal.
    public static final Layout PRETTY = new Layout() {
        @Override
        public String render(LogRecord rec, Theme theme) {
            return Text.prettyLine(rec, theme);
        }

        @Override
        public boolean caller() {
            return false;
        }
    };

    private static PatternLayout profile(Compat c) {
        synchronized (PROFILES) {
            return PROFILES.computeIfAbsent(c, k -> new PatternLayout(k.id()));
        }
    }

    /// A layout from a pattern or preset name, a function of the record, or a layout.
    @SuppressWarnings("unchecked")
    public static Layout of(Object x) {
        return switch (x) {
            case Layout l -> l;
            case String s -> new PatternLayout(s);
            case Function<?, ?> f -> (rec, theme) -> String.valueOf(((Function<LogRecord, ?>) f).apply(rec));
            default -> throw new IllegalArgumentException("not a layout: " + x);
        };
    }

    /// The layout a slot's keys ask for, or `null` for the sink's default.
    public static Layout build(Map<String, ?> k) {
        var layout = k.get("layout");
        if (layout != null) return layout instanceof String s && !s.contains("{") ? Kinds.layout(s) : of(layout);
        var format = k.get("format") == null ? null : String.valueOf(k.get("format"));
        var pattern = k.get("pattern");
        var jsonish =
                k.get("schema") != null || k.get("keys") != null || k.get("levels") != null || k.get("nest") != null;
        if (pattern != null && format != null && !format.equals("text"))
            throw new SerlogException.Config("`pattern` goes with `format: text`, not `" + format + "`");
        if (format == null || format.equals("text")) {
            if (pattern != null) return new PatternLayout(String.valueOf(pattern));
            return jsonish && format == null ? json(k) : null;
        }
        return switch (format) {
            case "json" -> json(k);
            case "logfmt" -> LOGFMT;
            case "pretty" -> PRETTY;
            default ->
                throw new SerlogException.Config("`format` takes text, logfmt, json or pretty, not `" + format + "`");
        };
    }

    @SuppressWarnings("unchecked")
    private static JsonLayout json(Map<String, ?> k) {
        var schema = k.get("schema");
        var levels = k.get("levels");
        var keys = (Map<String, String>) k.get("keys");
        return new JsonLayout(
                schema == null
                        ? JsonLayout.Schema.SERLOG
                        : schema instanceof JsonLayout.Schema s ? s : JsonLayout.Schema.parse(String.valueOf(schema)),
                keys == null ? Map.of() : keys,
                levels == null
                        ? null
                        : levels instanceof JsonLayout.Levels l ? l : JsonLayout.Levels.parse(String.valueOf(levels)),
                k.get("nest") == null ? null : String.valueOf(k.get("nest")));
    }
}
