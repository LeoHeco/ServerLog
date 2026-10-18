package io.github.leoheco.serlog.process;

import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.internal.Units;
import java.time.Duration;
import java.util.HashMap;
import java.util.Map;
import java.util.Optional;

/// Drops a record repeated within `within`; the next one kept carries `repeated`, the number dropped.
public final class Dedupe implements Processor {
    private static final class Seen {
        long at;
        long n;
    }

    private final long within;
    private final Map<String, Seen> seen = new HashMap<>();

    /// Drops repeats within `within`.
    public Dedupe(Duration within) {
        this.within = within.toNanos();
    }

    @Override
    public synchronized LogRecord process(LogRecord rec) {
        var key = rec.level().value() + "\0" + rec.target() + "\0" + rec.msg();
        var now = System.nanoTime();
        if (seen.size() > 10_000) seen.values().removeIf(s -> now - s.at >= within);
        var s = seen.get(key);
        if (s != null && now - s.at < within) {
            s.n++;
            return null;
        }
        var fresh = new Seen();
        fresh.at = now;
        seen.put(key, fresh);
        return s != null && s.n > 0 ? rec.with(Map.of("repeated", s.n)) : rec;
    }

    @Override
    public Optional<Map<String, Object>> options() {
        return Optional.of(Map.of("dedupe", Map.of("within", Units.showTime(within / 1e9))));
    }
}
