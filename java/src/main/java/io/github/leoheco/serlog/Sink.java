package io.github.leoheco.serlog;

import io.github.leoheco.serlog.layout.Layout;
import java.util.List;
import java.util.Map;
import java.util.Optional;

/// Somewhere entries go. Only [#emit] is required: queues, retries, health, alarms and layout come from the slot, so
/// a sink only delivers and throws with what went wrong. Throw [SerlogException.Rejected] when sending again will not
/// help.
@FunctionalInterface
public interface Sink {
    /// Writes one entry.
    void emit(Entry entry) throws Exception;

    /// Writes a batch, one by one unless the sink does better.
    default void emitBatch(List<Entry> entries) throws Exception {
        for (var e : entries) emit(e);
    }

    /// Whether it wants records at `level` from `target`.
    default boolean enabled(Level level, String target) {
        return true;
    }

    /// Writes out anything buffered.
    default void flush() throws Exception {}

    /// Releases what it holds, once its slot is removed or replaced.
    default void close() throws Exception {}

    /// The start-up check, run when it is registered.
    default void ready() throws Exception {}

    /// A resource only one sink may use, such as a file's path.
    default Optional<String> claim() {
        return Optional.empty();
    }

    /// Its kind, which also starts its default name.
    default String kind() {
        return "custom";
    }

    /// Whether it writes to a terminal.
    default boolean tty() {
        return false;
    }

    /// Its layout when the slot sets none; the profile's pattern unless it says otherwise.
    default Optional<Layout> defaultLayout() {
        return Optional.empty();
    }

    /// Its queue when the slot sets none; direct unless it says otherwise.
    default Optional<Queued> queued() {
        return Optional.empty();
    }

    /// The slot settings it carries, with the keys of a config entry.
    default Map<String, Object> slot() {
        return Map.of();
    }

    /// Its own config keys, for export, or empty when it cannot be exported.
    default Optional<Map<String, Object>> options() {
        return Optional.empty();
    }
}
