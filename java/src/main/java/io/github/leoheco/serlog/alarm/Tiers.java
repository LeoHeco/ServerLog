package io.github.leoheco.serlog.alarm;

import io.github.leoheco.serlog.internal.Units;
import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;

/// The built-in alarm: fixed tiers of sink names, `[[stderr]]` by default.
public final class Tiers extends Alarm {
    private final List<List<String>> tiers;
    private final Fallback fallback;
    private final Duration remind;

    /// Tiers tried in order, a fallback for when none took the incident, and how long a sink may stay down before
    /// it is reported again.
    public Tiers(List<List<String>> tiers, Fallback fallback, Duration remind) {
        if (tiers.isEmpty() || tiers.stream().anyMatch(List::isEmpty))
            throw new IllegalArgumentException("`tiers` needs at least one non-empty tier");
        this.tiers = tiers.stream().map(List::copyOf).toList();
        this.fallback = fallback;
        this.remind = remind;
    }

    /// Tiers that ignore what none took and remind after an hour.
    public Tiers(List<List<String>> tiers) {
        this(tiers, Fallback.IGNORE, Duration.ofHours(1));
    }

    /// The default: standard error only.
    public Tiers() {
        this(List.of(List.of("stderr")));
    }

    /// One tier per argument, each a single sink.
    public static Tiers of(String... sinks) {
        return new Tiers(List.of(sinks).stream().map(List::of).toList());
    }

    @Override
    public Fallback fallback() {
        return fallback;
    }

    @Override
    public Duration remind() {
        return remind;
    }

    /// The tiers.
    public List<List<String>> tiers() {
        return tiers;
    }

    @Override
    public List<List<String>> route(Incident incident) {
        return tiers;
    }

    @Override
    public List<String> names() {
        return tiers.stream().flatMap(List::stream).toList();
    }

    @Override
    public Optional<Map<String, Object>> options() {
        var o = new LinkedHashMap<String, Object>();
        o.put("tiers", tiers);
        if (fallback == Fallback.PANIC) o.put("on_fail", "panic");
        if (!remind.equals(Duration.ofHours(1))) o.put("remind", Units.showTime(remind));
        return Optional.of(o);
    }
}
