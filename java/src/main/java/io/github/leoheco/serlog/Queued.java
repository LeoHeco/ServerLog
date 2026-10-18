package io.github.leoheco.serlog;

import java.time.Duration;
import java.util.Locale;

/// The shape of a queue in front of a sink: it holds `size` records, written `batch` at a time or `wait` after the
/// first arrives; `delay` is the `wait` key.
///
/// @param size how many records it holds
/// @param batch how many are written at once
/// @param delay how long the first waits for others
/// @param full what a full queue does with a new record
public record Queued(int size, int batch, Duration delay, Full full) {
    /// What a full queue does with a new record.
    public enum Full {
        /// Drops it and counts it.
        DROP,
        /// Waits while the sink is up.
        BLOCK;

        /// A setting by its config name.
        public static Full parse(String name) {
            return valueOf(name.strip().toUpperCase(Locale.ROOT));
        }

        /// The config name.
        public String id() {
            return name().toLowerCase(Locale.ROOT);
        }
    }

    public Queued {
        if (size < 1 || batch < 1) throw new IllegalArgumentException("a queue holds at least one record");
    }

    /// A queue of `size`, 1000 a batch, one second, dropping when full.
    public static Queued of(int size) {
        return new Queued(size, 1000, Duration.ofSeconds(1), Full.DROP);
    }

    /// The same with another batch and wait.
    public Queued every(int batch, Duration delay) {
        return new Queued(size, batch, delay, full);
    }
}
