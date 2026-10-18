package io.github.leoheco.serlog;

import java.util.Map;
import java.util.TreeMap;

/// Colours by level; levels not listed keep their own.
///
/// @param overrides the colour for each level that has another than its own
public record Theme(Map<Level, Colour> overrides) {
    /// Every level in its own colour.
    public static final Theme PLAIN = new Theme(Map.of());

    public Theme {
        overrides = Map.copyOf(overrides);
    }

    /// A theme from config: level names to colour names.
    public static Theme parse(Map<?, ?> colours) {
        var out = new TreeMap<Level, Colour>();
        colours.forEach((l, c) -> out.put(Level.from(l), c instanceof Colour k ? k : Colour.parse(String.valueOf(c))));
        return new Theme(out);
    }

    /// The colour for `level`.
    public Colour of(Level level) {
        return overrides.getOrDefault(level, level.colour());
    }
}
