package io.github.leoheco.serlog.process;

import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.internal.Units;
import java.time.Duration;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Optional;

/// Per level and message, keeps the first `first` in each `per`, then one in `every`.
public final class Sample implements Processor {
    private static final class Count {
        long at;
        long n;
    }

    private final long first;
    private final long every;
    private final long per;
    private final Map<String, Count> seen = new HashMap<>();

    /// Keeps the first `first` of each kind in each `per`, then one in `every`.
    public Sample(long first, long every, Duration per) {
        this.first = first;
        this.every = Math.max(every, 1);
        this.per = per.toNanos();
    }

    @Override
    public synchronized LogRecord process(LogRecord rec) {
        var now = System.nanoTime();
        if (seen.size() > 10_000) seen.values().removeIf(c -> now - c.at >= per);
        var c = seen.computeIfAbsent(rec.level().value() + "\0" + rec.msg(), _ -> {
            var x = new Count();
            x.at = now;
            return x;
        });
        if (now - c.at >= per) {
            c.at = now;
            c.n = 0;
        }
        c.n++;
        return c.n <= first || (c.n - first) % every == 0 ? rec : null;
    }

    @Override
    public Optional<Map<String, Object>> options() {
        var o = new LinkedHashMap<String, Object>();
        o.put("first", first);
        o.put("every", every);
        o.put("per", Units.showTime(per / 1e9));
        return Optional.of(Map.of("sample", o));
    }
}
