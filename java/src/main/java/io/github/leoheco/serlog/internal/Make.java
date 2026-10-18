package io.github.leoheco.serlog.internal;

import io.github.leoheco.serlog.Field;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.layout.Text;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import java.util.function.Supplier;

/// Making records from logging calls: `{}` placeholders, deferred values and the caller.
public final class Make {
    private static final Set<String> OWN = Set.of(
            "io.github.leoheco.serlog.Logger",
            "io.github.leoheco.serlog.Logger$Event",
            "io.github.leoheco.serlog.Serlog",
            "io.github.leoheco.serlog.internal.Make",
            "io.github.leoheco.serlog.slf4j.SerlogLogger");
    private static final StackWalker WALKER = StackWalker.getInstance();

    private Make() {}

    /// A value worked out now, if it was deferred.
    public static Object value(Object v) {
        return v instanceof Supplier<?> s ? s.get() : v;
    }

    /// `msg` with each `{}` replaced by the next argument shown as text; `\{}` stays as `{}`.
    public static String format(String msg, Object[] args, int n) {
        if (n == 0 || msg.indexOf('{') < 0) return msg;
        var out = new StringBuilder(msg.length() + 16 * n);
        var a = 0;
        for (var i = 0; i < msg.length(); i++) {
            var c = msg.charAt(i);
            if (c == '\\' && i + 2 < msg.length() && msg.charAt(i + 1) == '{' && msg.charAt(i + 2) == '}') {
                out.append("{}");
                i += 2;
            } else if (c == '{' && i + 1 < msg.length() && msg.charAt(i + 1) == '}' && a < n) {
                out.append(Text.show(value(args[a++])));
                i++;
            } else out.append(c);
        }
        return out.toString();
    }

    /// A record from a call: bound fields, the message and its arguments, a trailing throwable becoming `error`.
    public static LogRecord record(
            Level level, String target, List<Field> bound, List<String> tags, String msg, Object[] args) {
        var n = args == null ? 0 : args.length;
        var fields = new ArrayList<Field>(bound.size() + 1);
        fields.addAll(bound);
        if (n > 0 && args[n - 1] instanceof Throwable t && placeholders(msg) < n) {
            fields.add(new Field("error", t));
            n--;
        }
        var rec = LogRecord.of(level, target, format(msg, args, n), fields);
        if (!tags.isEmpty()) rec = rec.withTags(tags);
        return caller(rec);
    }

    private static int placeholders(String msg) {
        var n = 0;
        for (var i = msg.indexOf("{}"); i >= 0; i = msg.indexOf("{}", i + 2))
            if (i == 0 || msg.charAt(i - 1) != '\\') n++;
        return n;
    }

    /// `rec` with the first caller outside serlog and SLF4J, when some slot may show it.
    public static LogRecord caller(LogRecord rec) {
        if (!Hub.caller()) return rec;
        return WALKER.walk(s -> s.dropWhile(
                        f -> OWN.contains(f.getClassName()) || f.getClassName().startsWith("org.slf4j."))
                .findFirst()
                .map(f -> rec.withCaller(
                        f.getClassName(), f.getMethodName(), f.getFileName(), Math.max(f.getLineNumber(), 0)))
                .orElse(rec));
    }
}
