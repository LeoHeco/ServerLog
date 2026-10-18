package io.github.leoheco.serlog.sink;

import io.github.leoheco.serlog.Entry;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.SinkBase;
import java.util.ArrayDeque;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.function.Predicate;

/// A ring buffer in memory, for tests and for looking back at recent records.
public class MemorySink extends SinkBase<MemorySink> {
    private record Kept(LogRecord record, String line) {}

    private final int size;
    private final ArrayDeque<Kept> kept = new ArrayDeque<>();

    /// Keeps the newest `size` records.
    public MemorySink(int size) {
        this.size = Math.max(size, 1);
    }

    /// Keeps the newest 1000 records.
    public MemorySink() {
        this(1000);
    }

    @Override
    public String kind() {
        return "memory";
    }

    @Override
    public synchronized void emit(Entry entry) {
        kept.add(new Kept(entry.record(), entry.line()));
        while (kept.size() > size) kept.poll();
    }

    /// The records kept, oldest first.
    public synchronized List<LogRecord> records() {
        return kept.stream().map(Kept::record).toList();
    }

    /// The lines kept, oldest first.
    public synchronized List<String> lines() {
        return kept.stream().map(Kept::line).toList();
    }

    /// The records for which `test` holds.
    public synchronized List<LogRecord> find(Predicate<LogRecord> test) {
        return kept.stream().map(Kept::record).filter(test).toList();
    }

    /// The last record with this message, or `null`.
    public synchronized LogRecord last(String msg) {
        return find(r -> r.msg().equals(msg)).reversed().stream().findFirst().orElse(null);
    }

    /// Forgets everything.
    public synchronized void clear() {
        kept.clear();
    }

    /// How many are kept.
    public synchronized int size() {
        return kept.size();
    }

    @Override
    public Optional<Map<String, Object>> options() {
        return Optional.of(Map.of("size", size));
    }
}
