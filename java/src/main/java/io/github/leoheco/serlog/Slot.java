package io.github.leoheco.serlog;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Set;

/// A sink with the settings of the slot it goes into: `Slot.of(sink).name("audit").queue(1000)`. The sink's own
/// slot settings come first, then these. A lambda taking an entry is a sink too.
public final class Slot implements SlotKeys<Slot> {
    /// The slot keys, as config files spell them.
    public static final Set<String> KEYS = Set.of(
            "name",
            "level",
            "filter",
            "process",
            "layout",
            "format",
            "pattern",
            "schema",
            "keys",
            "levels",
            "nest",
            "colour",
            "colours",
            "alarm_only",
            "on_fail",
            "queue",
            "batch",
            "wait",
            "full",
            "retries",
            "probe");

    private final Sink sink;
    private final Map<String, Object> keys;

    private Slot(Sink sink) {
        this.sink = sink;
        this.keys = new LinkedHashMap<>(sink.slot());
    }

    /// A slot for `sink`.
    public static Slot of(Sink sink) {
        return new Slot(sink);
    }

    @Override
    public Slot set(String key, Object value) {
        if (!KEYS.contains(key)) throw new IllegalArgumentException("unknown slot key `" + key + "`");
        keys.put(key, value);
        return this;
    }

    /// Sets several slot keys from a config entry.
    public Slot with(Map<String, ?> more) {
        more.forEach(this::set);
        return this;
    }

    /// The sink.
    public Sink sink() {
        return sink;
    }

    /// The slot keys set, sink's and own.
    public Map<String, Object> keys() {
        return Map.copyOf(keys);
    }
}
