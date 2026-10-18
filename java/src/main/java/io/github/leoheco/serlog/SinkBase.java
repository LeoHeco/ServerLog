package io.github.leoheco.serlog;

import java.util.LinkedHashMap;
import java.util.Map;

/// A sink that carries its own slot settings, set with the fluent methods of [SlotKeys].
///
/// @param <S> the sink's own type
public abstract class SinkBase<S extends SinkBase<S>> implements Sink, SlotKeys<S> {
    private final Map<String, Object> slot = new LinkedHashMap<>();

    @SuppressWarnings("unchecked")
    @Override
    public S set(String key, Object value) {
        if (!Slot.KEYS.contains(key)) throw new IllegalArgumentException("unknown slot key `" + key + "`");
        slot.put(key, value);
        return (S) this;
    }

    @Override
    public Map<String, Object> slot() {
        return Map.copyOf(slot);
    }
}
