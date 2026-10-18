package io.github.leoheco.serlog;

import java.time.Instant;
import java.util.Locale;

/// A slot's state, for monitoring.
///
/// @param name the slot's name
/// @param kind its sink's kind
/// @param health whether it works
/// @param skipped records skipped while it was down
/// @param dropped records its queue dropped or the sink refused
/// @param suppressed records made while its sink ran, dropped against loops
/// @param since when its current health began
/// @param error its latest error while not up, or `null`
public record Status(
        String name,
        String kind,
        Health health,
        long skipped,
        long dropped,
        long suppressed,
        Instant since,
        String error) {
    /// Whether a sink is working.
    public enum Health {
        UP,
        DOWN,
        /// One write after a failure decides.
        PROBING;

        /// The name used in the shared vectors.
        public String id() {
            return name().toLowerCase(Locale.ROOT);
        }
    }
}
