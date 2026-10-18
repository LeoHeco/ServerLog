package io.github.leoheco.serlog;

import java.time.Instant;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.atomic.AtomicLong;

/// One log event as every sink sees it. Records are immutable: a processor that changes one makes a copy, so every
/// other slot still sees the original.
///
/// @param level the severity
/// @param target where it comes from, such as a class or subsystem
/// @param msg the message
/// @param fields named values, in order; a later field of the same name wins
/// @param tags labels for routing and matching
/// @param module the calling class, or `null`
/// @param func the calling method, or `null`
/// @param file the calling source file, or `null`
/// @param line the calling line, or 0 when unknown
/// @param thread the thread name
/// @param tid the thread id
/// @param spans the span chain, or `null`
/// @param time when it was made
/// @param seq its number, counting from 0 since start
public record LogRecord(
        Level level,
        String target,
        String msg,
        List<Field> fields,
        List<String> tags,
        String module,
        String func,
        String file,
        int line,
        String thread,
        long tid,
        String spans,
        Instant time,
        long seq) {
    private static final AtomicLong SEQ = new AtomicLong();

    public LogRecord {
        Objects.requireNonNull(level, "level");
        Objects.requireNonNull(target, "target");
        Objects.requireNonNull(msg, "msg");
        fields = List.copyOf(fields);
        tags = List.copyOf(tags);
        Objects.requireNonNull(time, "time");
    }

    /// A record made now, on this thread.
    public static LogRecord of(Level level, String target, String msg) {
        return of(level, target, msg, List.of());
    }

    /// A record with fields, made now, on this thread.
    public static LogRecord of(Level level, String target, String msg, List<Field> fields) {
        var t = Thread.currentThread();
        var name = t.getName().isEmpty() ? (t.isVirtual() ? "virtual-" : "thread-") + t.threadId() : t.getName();
        return new LogRecord(
                level,
                target,
                msg,
                fields,
                List.of(),
                null,
                null,
                null,
                0,
                name,
                t.threadId(),
                null,
                Instant.now(),
                SEQ.getAndIncrement());
    }

    /// The last field called `key`, or `null`.
    public Object field(String key) {
        for (var f : fields.reversed()) if (f.key().equals(key)) return f.value();
        return null;
    }

    /// Whether a field called `key` is present.
    public boolean has(String key) {
        return fields.stream().anyMatch(f -> f.key().equals(key));
    }

    /// The fields as a map, later ones winning.
    public Map<String, Object> fieldMap() {
        var out = new LinkedHashMap<String, Object>();
        for (var f : fields) out.put(f.key(), f.value());
        return out;
    }

    /// A copy with another message.
    public LogRecord withMsg(String msg) {
        return new LogRecord(level, target, msg, fields, tags, module, func, file, line, thread, tid, spans, time, seq);
    }

    /// A copy with another level.
    public LogRecord withLevel(Level level) {
        return new LogRecord(level, target, msg, fields, tags, module, func, file, line, thread, tid, spans, time, seq);
    }

    /// A copy with other fields.
    public LogRecord withFields(List<Field> fields) {
        return new LogRecord(level, target, msg, fields, tags, module, func, file, line, thread, tid, spans, time, seq);
    }

    /// A copy with other tags.
    public LogRecord withTags(List<String> tags) {
        return new LogRecord(level, target, msg, fields, tags, module, func, file, line, thread, tid, spans, time, seq);
    }

    /// A copy made at another time.
    public LogRecord withTime(Instant time) {
        return new LogRecord(level, target, msg, fields, tags, module, func, file, line, thread, tid, spans, time, seq);
    }

    /// A copy with another caller.
    public LogRecord withCaller(String module, String func, String file, int line) {
        return new LogRecord(level, target, msg, fields, tags, module, func, file, line, thread, tid, spans, time, seq);
    }

    /// A copy with another thread.
    public LogRecord withThread(String thread, long tid) {
        return new LogRecord(level, target, msg, fields, tags, module, func, file, line, thread, tid, spans, time, seq);
    }

    /// A copy with these fields set, replacing fields of the same name.
    public LogRecord with(Map<String, ?> set) {
        var out = new ArrayList<Field>(fields.size() + set.size());
        for (var f : fields) if (!set.containsKey(f.key())) out.add(f);
        set.forEach((k, v) -> out.add(new Field(k, v)));
        return withFields(out);
    }
}
