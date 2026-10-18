package io.github.leoheco.serlog.alarm;

import io.github.leoheco.serlog.Field;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.layout.Clock;
import java.time.Duration;
import java.time.Instant;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.stream.Collectors;

/// Routes incidents to sinks. Extend it and override [#route] to choose the tiers; the alarm task walks them, leaves
/// out the failing sink and any that are resting, delivers within a tier at once and stops at the first tier where
/// one sink took it. Subclasses cannot cause loops or storms.
public abstract class Alarm {
    /// What an alarm does when no tier took the incident.
    public enum Fallback {
        PANIC,
        IGNORE;

        /// A fallback by its config name.
        public static Fallback parse(String name) {
            try {
                return valueOf(name.strip().toUpperCase(Locale.ROOT));
            } catch (IllegalArgumentException e) {
                throw new IllegalArgumentException("an alarm's `on_fail` is panic or ignore, not `" + name + "`", e);
            }
        }

        /// The config name.
        public String id() {
            return name().toLowerCase(Locale.ROOT);
        }
    }

    /// What to do when no tier took it.
    public Fallback fallback() {
        return Fallback.IGNORE;
    }

    /// How long a sink may stay down before it is reported again.
    public Duration remind() {
        return Duration.ofHours(1);
    }

    /// The tiers for this incident, tried in order; `*` stands for every sink on the log route.
    public abstract List<List<String>> route(Incident incident);

    /// The record sent; asked again before each tier, so later tiers see earlier failures.
    public LogRecord describe(Incident i) {
        return standard(i);
    }

    /// Every sink name this alarm may route to, checked at start-up.
    public List<String> names() {
        return List.of();
    }

    /// The config entry that rebuilds this alarm, or empty when it cannot be exported.
    public Optional<Map<String, Object>> options() {
        return Optional.empty();
    }

    /// The default record for an incident.
    public static LogRecord standard(Incident i) {
        var gone = Duration.between(i.since(), Instant.now()).toSeconds();
        var level = i.phase() == Incident.Phase.RESOLVED ? Level.INFO : Level.ERROR;
        var msg = switch (i.phase()) {
            case OPENED -> "sink `" + i.sink() + "` failed: " + i.error();
            case REMINDER -> "sink `" + i.sink() + "` is still failing: " + i.error();
            case RESOLVED -> "sink `" + i.sink() + "` works again after " + gone + "s";
        };
        return LogRecord.of(
                level,
                "serlog",
                msg,
                List.of(
                        Field.of("sink", i.sink()),
                        Field.of("kind", i.kind()),
                        Field.of("phase", i.phase().id()),
                        Field.of(
                                "failures",
                                i.failures().stream()
                                        .map(f -> f.sink() + ": " + f.error())
                                        .collect(Collectors.joining("; "))),
                        Field.of("skipped", i.skipped()),
                        Field.of("dropped", i.dropped()),
                        Field.of("since", Clock.iso(i.since(), 9))));
    }
}
