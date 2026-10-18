package io.github.leoheco.serlog.layout;

import io.github.leoheco.serlog.Colour;
import io.github.leoheco.serlog.Field;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.Theme;
import io.github.leoheco.serlog.internal.Hub;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.lang.reflect.Array;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.time.temporal.TemporalAccessor;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Pattern;
import java.util.stream.Collectors;
import java.util.stream.IntStream;

/// Text pieces shared by the layouts and sinks: quoting, values, field lists, logfmt and the pretty layout.
public final class Text {
    private static final Pattern BARE = Pattern.compile("[\\s\"=]");

    private Text() {}

    /// `v` as a JSON string, escaped as every serlog package escapes it.
    public static String quote(Object v) {
        var s = String.valueOf(v);
        var out = new StringBuilder(s.length() + 2).append('"');
        s.codePoints().forEach(c -> {
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (c < 0x20) out.append("\\u%04x".formatted(c));
                    else out.appendCodePoint(c);
                }
            }
        });
        return out.append('"').toString();
    }

    /// `v` bare, or quoted when it is empty or holds whitespace, quotes or `=`.
    public static String logfmt(Object v) {
        var s = show(v);
        return !s.isEmpty() && !BARE.matcher(s).find() ? s : quote(s);
    }

    /// What `e` was caused by, then what that was caused by, and so on.
    public static List<Throwable> causes(Throwable e) {
        var seen = Collections.newSetFromMap(new IdentityHashMap<Throwable, Boolean>());
        seen.add(e);
        var out = new ArrayList<Throwable>();
        for (var c = e.getCause(); c != null && seen.add(c); c = c.getCause()) out.add(c);
        return out;
    }

    private static List<?> items(Object v) {
        return v instanceof Collection<?> c
                ? new ArrayList<>(c)
                : IntStream.range(0, Array.getLength(v))
                        .mapToObj(i -> Array.get(v, i))
                        .toList();
    }

    private static boolean listish(Object v) {
        return v instanceof Collection<?> || v.getClass().isArray();
    }

    /// A value as text: `null`, `true`, `[a, b]`, `{k=v}`, errors with their causes.
    public static String show(Object v) {
        return switch (v) {
            case null -> "null";
            case String s -> s;
            case Throwable e -> {
                var parts = new ArrayList<String>();
                parts.add(SerlogException.message(e));
                for (var c : causes(e)) parts.add(SerlogException.message(c));
                yield String.join(": ", parts);
            }
            case Map<?, ?> m ->
                m.entrySet().stream()
                        .map(x -> x.getKey() + "=" + show(x.getValue()))
                        .collect(Collectors.joining(", ", "{", "}"));
            case Object o
            when listish(o) -> items(o).stream().map(Text::show).collect(Collectors.joining(", ", "[", "]"));
            default -> String.valueOf(v);
        };
    }

    /// A stack trace as text, or `null` when there is none.
    public static String stack(Throwable e) {
        if (e.getStackTrace().length == 0) return null;
        var w = new StringWriter();
        e.printStackTrace(new PrintWriter(w));
        return w.toString().stripTrailing();
    }

    /// A value as JSON.
    public static String json(Object v) {
        return switch (v) {
            case null -> "null";
            case Boolean b -> b.toString();
            case Integer _, Long _, Short _, Byte _, BigInteger _, BigDecimal _ -> v.toString();
            case Double d -> d.isInfinite() || d.isNaN() ? quote(d) : number(d);
            case Float f -> f.isInfinite() || f.isNaN() ? quote(f) : number(f.doubleValue());
            case Number n -> n.toString();
            case String s -> quote(s);
            case Throwable e -> {
                var out = new StringBuilder("{\"message\":").append(quote(SerlogException.message(e)));
                var c = causes(e);
                if (!c.isEmpty())
                    out.append(",\"causes\":")
                            .append(c.stream()
                                    .map(x -> quote(SerlogException.message(x)))
                                    .collect(Collectors.joining(",", "[", "]")));
                var st = stack(e);
                if (st != null) out.append(",\"stack\":").append(quote(st));
                yield out.append('}').toString();
            }
            case Map<?, ?> m ->
                m.entrySet().stream()
                        .map(x -> quote(x.getKey()) + ":" + json(x.getValue()))
                        .collect(Collectors.joining(",", "{", "}"));
            case TemporalAccessor t -> quote(t);
            case Object o
            when listish(o) -> items(o).stream().map(Text::json).collect(Collectors.joining(",", "[", "]"));
            default -> quote(v);
        };
    }

    private static String number(double d) {
        var s = BigDecimal.valueOf(d).toPlainString();
        return s.contains(".") ? s : s + ".0";
    }

    /// Each field, maps spread into `key.sub`, leaving out the keys in `skip`.
    public static List<Field> flat(List<Field> fields, Set<String> skip) {
        var out = new ArrayList<Field>(fields.size());
        for (var f : fields) if (!skip.contains(f.key())) walk(f.key(), f.value(), out);
        return out;
    }

    private static void walk(String key, Object v, List<Field> out) {
        if (v instanceof Map<?, ?> m) m.forEach((k, x) -> walk(key + "." + k, x, out));
        else out.add(new Field(key, v));
    }

    /// ` key=value` per field.
    public static String fieldText(LogRecord rec, boolean quoted, Set<String> skip) {
        var out = new StringBuilder();
        for (var f : flat(rec.fields(), skip))
            out.append(' ').append(f.key()).append('=').append(quoted ? logfmt(f.value()) : show(f.value()));
        return out.toString();
    }

    /// `time=… level=… target=… msg=… key=value…`
    public static String logfmtLine(LogRecord rec) {
        return "time=" + Clock.iso(rec.time(), 0) + " level=" + rec.level().id() + " target=" + logfmt(rec.target())
                + " msg=" + logfmt(rec.msg()) + fieldText(rec, true, Set.of());
    }

    private static String padEnd(String s, int w) {
        return s.length() >= w ? s : s + " ".repeat(w - s.length());
    }

    /// A first line with time, level, target and message, then one aligned line per field.
    public static String prettyLine(LogRecord rec, Theme theme) {
        var on = theme != null;
        var colour = on ? theme.of(rec.level()) : Colour.PLAIN;
        var out = new StringBuilder()
                .append(Colour.GREY.paint(on, Clock.write("", rec.time())))
                .append(' ')
                .append(colour.paint(on, padEnd(rec.level().label(Hub.compat()), 5)))
                .append(' ')
                .append(Colour.WHITE.paint(on, rec.target()))
                .append(' ')
                .append(rec.msg());
        var rows = flat(rec.fields(), Set.of());
        var width = rows.stream().mapToInt(f -> f.key().length()).max().orElse(0);
        for (var f : rows)
            out.append("\n    ")
                    .append(Colour.CYAN.paint(on, padEnd(f.key(), width)))
                    .append(" = ")
                    .append(show(f.value()));
        return out.toString();
    }
}
