package io.github.leoheco.serlog.process;

import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.layout.Text;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;

/// Keeps a record only when every condition given holds; with `exclude`, drops it then instead.
///
/// @param tags tags the record must carry
/// @param fields fields it must have, compared as text
/// @param level the lowest level, or `null`
/// @param exclude whether a match drops the record instead of keeping it
public record Match(List<String> tags, Map<String, String> fields, Level level, boolean exclude) implements Processor {
    public Match {
        tags = List.copyOf(tags);
        fields = Collections.unmodifiableMap(new LinkedHashMap<>(fields));
    }

    /// Keeps records carrying every one of these tags.
    public static Match tags(String... tags) {
        return new Match(List.of(tags), Map.of(), null, false);
    }

    /// Keeps records with these fields, compared as text.
    public static Match fields(Map<String, ?> fields) {
        var f = new LinkedHashMap<String, String>();
        fields.forEach((k, v) -> f.put(k, Text.show(v)));
        return new Match(List.of(), f, null, false);
    }

    /// Keeps records at `level` and above.
    public static Match level(Level level) {
        return new Match(List.of(), Map.of(), level, false);
    }

    /// The same conditions, dropping what matches.
    public Match negate() {
        return new Match(tags, fields, level, !exclude);
    }

    /// Whether every condition holds.
    public boolean holds(LogRecord rec) {
        if (level != null && !rec.level().atLeast(level)) return false;
        if (!rec.tags().containsAll(tags)) return false;
        for (var e : fields.entrySet())
            if (!rec.has(e.getKey()) || !Text.show(rec.field(e.getKey())).equals(e.getValue())) return false;
        return true;
    }

    @Override
    public LogRecord process(LogRecord rec) {
        return holds(rec) != exclude ? rec : null;
    }

    @Override
    public Optional<Map<String, Object>> options() {
        var o = new LinkedHashMap<String, Object>();
        if (!tags.isEmpty()) o.put("tags", tags);
        if (!fields.isEmpty()) o.put("fields", new LinkedHashMap<>(fields));
        if (level != null) o.put("level", level.id());
        return Optional.of(Map.of(exclude ? "exclude" : "match", o));
    }
}
