package io.github.leoheco.serlog.process;

import io.github.leoheco.serlog.LogRecord;
import java.util.List;
import java.util.Map;
import java.util.Optional;

/// A step a record goes through before it is written: return it, a changed copy, or `null` to drop it. Records are
/// immutable, so every other slot still sees the original.
@FunctionalInterface
public interface Processor {
    /// The record to go on with, or `null` to drop it.
    LogRecord process(LogRecord rec);

    /// The config entry that rebuilds this step, such as `{dedupe: {within: 5s}}`, or empty when it cannot be
    /// exported.
    default Optional<Map<String, Object>> options() {
        return Optional.empty();
    }

    /// `rec` after every step, or `null` once one dropped it.
    static LogRecord run(List<? extends Processor> steps, LogRecord rec) {
        var cur = rec;
        for (var s : steps) {
            cur = s.process(cur);
            if (cur == null) return null;
        }
        return cur;
    }
}
