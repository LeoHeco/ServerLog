package io.github.leoheco.serlog.process;

import io.github.leoheco.serlog.LogRecord;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Optional;

/// A token bucket: `rate` records a second, up to `burst` at once.
public final class Limit implements Processor {
    private final double rate;
    private final double burst;
    private double tokens;
    private long last = System.nanoTime();

    /// `rate` a second, at most `burst` at once.
    public Limit(double rate, double burst) {
        this.rate = rate;
        this.burst = Math.max(burst, 1);
        this.tokens = this.burst;
    }

    /// `rate` a second, at most as many at once.
    public Limit(double rate) {
        this(rate, rate);
    }

    @Override
    public synchronized LogRecord process(LogRecord rec) {
        var now = System.nanoTime();
        tokens = Math.min(burst, tokens + (now - last) / 1e9 * rate);
        last = now;
        if (tokens < 1) return null;
        tokens--;
        return rec;
    }

    @Override
    public Optional<Map<String, Object>> options() {
        var o = new LinkedHashMap<String, Object>();
        o.put("rate", rate == Math.rint(rate) ? (Object) (long) rate : rate);
        o.put("burst", burst == Math.rint(burst) ? (Object) (long) burst : burst);
        return Optional.of(Map.of("limit", o));
    }
}
