package io.github.leoheco.serlog.layout;

import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.Theme;

/// How a record becomes one line.
@FunctionalInterface
public interface Layout {
    /// `rec` as one line; `theme` is `null` when the line is not coloured.
    String render(LogRecord rec, Theme theme);

    /// Whether lines may show the caller, which costs a stack walk per record to find.
    default boolean caller() {
        return true;
    }
}
