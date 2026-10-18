package io.github.leoheco.serlog.alarm;

import java.time.Instant;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/// A sink's failure, from the first error to recovery. The alarm walk adds every failed delivery to `failures`.
///
/// @param sink the failing sink's name
/// @param kind its kind
/// @param phase which report this is
/// @param error its latest error
/// @param failures the failed writes and deliveries so far, the sink's own first
/// @param skipped records skipped while it was down
/// @param dropped records its queue dropped
/// @param since when the failure began
public record Incident(
        String sink,
        String kind,
        Phase phase,
        String error,
        List<Failure> failures,
        long skipped,
        long dropped,
        Instant since) {
    /// Which report of an incident this is.
    public enum Phase {
        OPENED,
        REMINDER,
        RESOLVED;

        /// The config and field name.
        public String id() {
            return name().toLowerCase(Locale.ROOT);
        }

        /// A phase by its name.
        public static Phase parse(String name) {
            return valueOf(name.strip().toUpperCase(Locale.ROOT));
        }
    }

    /// One failed write or delivery.
    ///
    /// @param sink the sink that failed
    /// @param error what went wrong
    /// @param time when
    public record Failure(String sink, String error, Instant time) {}

    public Incident {
        failures = new ArrayList<>(failures);
    }

    /// A fresh incident; its failures start with the sink's own unless it is resolved.
    public static Incident of(
            String sink, String kind, Phase phase, String error, long skipped, long dropped, Instant since) {
        var failures = phase == Phase.RESOLVED ? List.<Failure>of() : List.of(new Failure(sink, error, Instant.now()));
        return new Incident(sink, kind, phase, error, failures, skipped, dropped, since);
    }

    /// A fresh incident with no counts, begun now.
    public static Incident of(String sink, String kind, Phase phase, String error) {
        return of(sink, kind, phase, error, 0, 0, Instant.now());
    }
}
