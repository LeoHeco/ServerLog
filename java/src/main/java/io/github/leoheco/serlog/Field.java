package io.github.leoheco.serlog;

import java.util.Objects;

/// A named value attached to a record. Values keep their types: null, booleans, numbers, strings, lists, maps and
/// errors are shown and serialised as such; anything else as its string.
///
/// @param key the name
/// @param value the value, which may be null
public record Field(String key, Object value) {
    public Field {
        Objects.requireNonNull(key, "key");
    }

    /// A field.
    public static Field of(String key, Object value) {
        return new Field(key, value);
    }
}
