package io.github.leoheco.serlog;

import io.github.leoheco.serlog.layout.Layout;
import io.github.leoheco.serlog.layout.Layouts;
import java.util.HashMap;
import java.util.Map;

/// What a sink is handed: the record, and the line its slot's layout makes of it, laid out on first use and shared
/// with other slots using the same layout.
public final class Entry {
    private final LogRecord record;
    private final Layout layout;
    private final Theme theme;
    private final Map<Layout, Map<Theme, String>> cache;
    private String line;

    /// An entry laid out by `layout`, uncoloured.
    public Entry(LogRecord record, Layout layout) {
        this(record, layout, null, null);
    }

    /// An entry in the profile's pattern.
    public Entry(LogRecord record) {
        this(record, Layouts.INHERIT);
    }

    /// An entry coloured by `theme` unless it is `null`, sharing lines through `cache` when given.
    public Entry(LogRecord record, Layout layout, Theme theme, Map<Layout, Map<Theme, String>> cache) {
        this.record = record;
        this.layout = layout;
        this.theme = theme;
        this.cache = cache;
    }

    /// The record.
    public LogRecord record() {
        return record;
    }

    /// Whether the line is coloured.
    public boolean coloured() {
        return theme != null;
    }

    /// The line.
    public String line() {
        if (line == null) {
            var byTheme = cache == null ? null : cache.computeIfAbsent(layout, _ -> new HashMap<>());
            line = byTheme == null ? null : byTheme.get(theme);
            if (line == null) {
                line = layout.render(record, theme);
                if (byTheme != null) byTheme.put(theme, line);
            }
        }
        return line;
    }

    @Override
    public String toString() {
        return line();
    }
}
