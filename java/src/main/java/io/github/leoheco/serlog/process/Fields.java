package io.github.leoheco.serlog.process;

import io.github.leoheco.serlog.LogRecord;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Optional;

/// Sets the same fields on every record.
///
/// @param fields the fields, in order
public record Fields(Map<String, Object> fields) implements Processor {
    public Fields {
        fields = Collections.unmodifiableMap(new LinkedHashMap<>(fields));
    }

    @Override
    public LogRecord process(LogRecord rec) {
        return fields.isEmpty() ? rec : rec.with(fields);
    }

    @Override
    public Optional<Map<String, Object>> options() {
        return Optional.of(Map.of("fields", new LinkedHashMap<>(fields)));
    }
}
