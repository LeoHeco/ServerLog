package io.github.leoheco.serlog.layout;

import io.github.leoheco.serlog.Compat;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.Meta;
import io.github.leoheco.serlog.Theme;
import io.github.leoheco.serlog.internal.Hub;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Pattern;

/// A line template: `{time} {level:<5} {target}: {msg}{fields}`. Holes are `{token}` or `{token:spec}`; `{{` and
/// `}}` are literal braces. Every token but `time` may end with `<N`, `>N` or `^N` to pad, then `.M` to cut. A
/// preset name such as `tracing` stands for that logger's pattern and shows levels the way it does.
public final class PatternLayout implements Layout {
    private sealed interface Piece {}

    private record Lit(String text) implements Piece {}

    private record Hole(String kind, Object option, char align, int width, int cut) implements Piece {}

    private static final Set<String> PLAIN = Set.of(
            "module",
            "func",
            "line",
            "thread",
            "tid",
            "task",
            "pid",
            "host",
            "app",
            "env",
            "instance",
            "tags",
            "elapsed",
            "seq",
            "spans");
    private static final Map<String, String> OPTIONS =
            Map.of("level", "lower", "file", "base", "msg", "q", "fields", "q", "error", "stack");
    private static final Set<String> CALLER = Set.of("module", "func", "file", "line");
    private static final Pattern TOKEN = Pattern.compile("\\{\\{|}}|\\{([^}]*)}|}");
    private static final Pattern FIT = Pattern.compile("(?:([<>^])(\\d+))?(?:\\.(\\d+))?");
    private static final Pattern TILDE = Pattern.compile("~(\\d+)");
    private static final long PID = ProcessHandle.current().pid();

    private final String source;
    private final List<Piece> pieces;
    private final boolean showsError;
    private final boolean caller;
    private final Compat labels;

    /// Parses a pattern, or a preset name such as `tracing`.
    public PatternLayout(String source) {
        var named = Compat.preset(source);
        var s = named != null ? named.pattern() : source;
        this.source = source;
        this.labels = named;
        var out = new ArrayList<Piece>();
        var lit = new StringBuilder();
        var at = 0;
        var m = TOKEN.matcher(s);
        while (m.find()) {
            lit.append(s, at, m.start());
            at = m.end();
            var g = m.group();
            if (g.equals("{{") || g.equals("}}")) {
                lit.append(g.charAt(0));
                continue;
            }
            if (g.equals("}")) throw bad(s, "stray `}`");
            if (!lit.isEmpty()) out.add(new Lit(lit.toString()));
            lit.setLength(0);
            var hole = m.group(1);
            var colon = hole.indexOf(':');
            out.add(
                    colon < 0
                            ? hole(hole.strip(), "", s)
                            : hole(hole.substring(0, colon).strip(), hole.substring(colon + 1), s));
        }
        if (s.substring(at).contains("{")) throw bad(s, "unclosed `{`");
        lit.append(s.substring(at));
        if (!lit.isEmpty()) out.add(new Lit(lit.toString()));
        this.pieces = List.copyOf(out);
        this.showsError =
                out.stream().anyMatch(p -> p instanceof Hole h && h.kind().equals("error"));
        this.caller = out.stream().anyMatch(p -> p instanceof Hole h && CALLER.contains(h.kind()));
    }

    private static IllegalArgumentException bad(String s, String why) {
        return new IllegalArgumentException("pattern `" + s + "`: " + why);
    }

    private static Hole hole(String name, String spec, String s) {
        if (name.equals("time")) return new Hole("time", spec, ' ', 0, -1);
        Object option = false;
        var rest = spec;
        var word = OPTIONS.get(name);
        if (name.equals("target") && spec.startsWith("~")) {
            var t = TILDE.matcher(spec);
            if (!t.lookingAt()) throw bad(s, "bad `~` in `" + spec + "`");
            option = Integer.parseInt(t.group(1));
            rest = spec.substring(t.end());
        } else if (word != null) {
            option = spec.startsWith(word);
            if ((boolean) option) rest = spec.substring(word.length());
        } else if (!PLAIN.contains(name) && !name.equals("target")) {
            throw bad(s, "unknown token `" + name + "`");
        }
        var f = FIT.matcher(rest);
        if (!f.matches()) throw bad(s, "bad spec `" + spec + "` for `" + name + "`");
        return new Hole(
                name,
                option,
                f.group(1) == null ? ' ' : f.group(1).charAt(0),
                f.group(2) == null ? 0 : Integer.parseInt(f.group(2)),
                f.group(3) == null ? -1 : Integer.parseInt(f.group(3)));
    }

    /// Shortens leading segments of `target` to their first letter until it fits in `n`.
    public static String shorten(String target, int n) {
        if (target.length() <= n) return target;
        var sep = target.contains("::") ? "::" : ".";
        var parts = target.split(Pattern.quote(sep), -1);
        var out = parts.clone();
        for (var i = 0; i < parts.length - 1 && String.join(sep, out).length() > n; i++)
            out[i] = parts[i].isEmpty() ? "" : parts[i].substring(0, 1);
        return String.join(sep, out);
    }

    private static String align(String text, char how, int width) {
        var pad = Math.max(width - text.codePointCount(0, text.length()), 0);
        return switch (how) {
            case '<' -> text + " ".repeat(pad);
            case '>' -> " ".repeat(pad) + text;
            case '^' -> " ".repeat(pad / 2) + text + " ".repeat(pad - pad / 2);
            default -> text;
        };
    }

    /// The text it was parsed from, a preset name included.
    public String source() {
        return source;
    }

    @Override
    public boolean caller() {
        return caller;
    }

    @Override
    public String render(LogRecord rec, Theme theme) {
        var out = new StringBuilder(64);
        for (var p : pieces) {
            switch (p) {
                case Lit(var text) -> out.append(text);
                case Hole h -> {
                    var text = token(h, rec);
                    if (h.cut() >= 0 && text.codePointCount(0, text.length()) > h.cut())
                        text = text.substring(0, text.offsetByCodePoints(0, h.cut()));
                    text = align(text, h.align(), h.width());
                    if (h.kind().equals("level") && theme != null)
                        text = theme.of(rec.level()).paint(true, text);
                    out.append(text);
                }
            }
        }
        return out.toString();
    }

    private String token(Hole h, LogRecord rec) {
        var on = h.option() instanceof Boolean b && b;
        return switch (h.kind()) {
            case "time" -> Clock.write((String) h.option(), rec.time());
            case "level" -> on ? rec.level().id() : rec.level().label(labels != null ? labels : Hub.compat());
            case "target" -> h.option() instanceof Integer n ? shorten(rec.target(), n) : rec.target();
            case "module" -> rec.module() != null ? rec.module() : rec.target();
            case "func" -> rec.func() != null ? rec.func() : "?";
            case "file" -> rec.file() == null ? "?" : on ? base(rec.file()) : rec.file();
            case "line" -> rec.line() > 0 ? String.valueOf(rec.line()) : "?";
            case "thread" -> rec.thread();
            case "tid" -> String.valueOf(rec.tid());
            case "task" -> "?";
            case "pid" -> String.valueOf(PID);
            case "host" -> Meta.host();
            case "app" -> Hub.meta().app();
            case "env" -> Hub.meta().env();
            case "instance" -> Hub.meta().instance();
            case "msg" -> on ? Text.logfmt(rec.msg()) : rec.msg();
            case "fields" -> Text.fieldText(rec, on, showsError ? Set.of("error") : Set.of());
            case "tags" -> String.join(",", rec.tags());
            case "error" -> {
                if (!rec.has("error")) yield "";
                var e = rec.field("error");
                var st = on && e instanceof Throwable t ? Text.stack(t) : null;
                yield Text.show(e) + (st == null ? "" : "\n" + st);
            }
            case "elapsed" -> String.valueOf(Clock.elapsed());
            case "seq" -> String.valueOf(rec.seq());
            case "spans" -> rec.spans() != null && !rec.spans().isEmpty() ? rec.spans() + ": " : "";
            default -> "";
        };
    }

    private static String base(String file) {
        return file.substring(Math.max(file.lastIndexOf('/'), file.lastIndexOf('\\')) + 1);
    }

    @Override
    public String toString() {
        return source;
    }

    @Override
    public boolean equals(Object o) {
        return o instanceof PatternLayout p && p.source.equals(source);
    }

    @Override
    public int hashCode() {
        return source.hashCode();
    }
}
