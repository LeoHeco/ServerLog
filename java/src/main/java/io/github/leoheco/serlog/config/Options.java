package io.github.leoheco.serlog.config;

import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.internal.Units;
import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

/// A config entry's own keys, handed to a registered factory. Every key must be read: one left unread is an error
/// that names it.
public final class Options {
    private final String what;
    private final Map<String, Object> keys;
    private final Set<String> read = new LinkedHashSet<>();

    /// The keys of `what`, such as `file sink`.
    public Options(String what, Map<String, ?> keys) {
        this.what = what;
        this.keys = new LinkedHashMap<>(keys);
    }

    private SerlogException.Config bad(String key, String why) {
        return new SerlogException.Config(what + ": `" + key + "` " + why);
    }

    /// The raw value, if given.
    public Optional<Object> get(String key) {
        read.add(key);
        return Optional.ofNullable(keys.get(key));
    }

    /// Whether the key is given.
    public boolean has(String key) {
        return keys.containsKey(key);
    }

    /// A required string.
    public String string(String key) {
        return string(key, null).orElseThrow(() -> bad(key, "is required"));
    }

    /// A string, if given; `alias` is another accepted name.
    public Optional<String> string(String key, String alias) {
        var v = get(key).or(() -> alias == null ? Optional.empty() : get(alias));
        return v.map(String::valueOf);
    }

    /// A whole number, if given.
    public Optional<Long> integer(String key) {
        return get(key).map(v -> {
            if (v instanceof Number n && n.doubleValue() == Math.rint(n.doubleValue())) return n.longValue();
            throw bad(key, "takes a whole number, not `" + v + "`");
        });
    }

    /// A number, if given.
    public Optional<Double> number(String key) {
        return get(key).map(v -> {
            if (v instanceof Number n) return n.doubleValue();
            throw bad(key, "takes a number, not `" + v + "`");
        });
    }

    /// A boolean, if given.
    public Optional<Boolean> bool(String key) {
        return get(key).map(v -> {
            if (v instanceof Boolean b) return b;
            throw bad(key, "takes true or false, not `" + v + "`");
        });
    }

    /// A duration such as `5s`, if given.
    public Optional<Duration> duration(String key) {
        return get(key).map(v -> {
            try {
                return Units.duration(v);
            } catch (IllegalArgumentException e) {
                throw bad(key, SerlogException.message(e));
            }
        });
    }

    /// A size such as `64MiB`, if given.
    public Optional<Long> size(String key) {
        return get(key).map(v -> {
            try {
                return Units.size(v);
            } catch (IllegalArgumentException e) {
                throw bad(key, SerlogException.message(e));
            }
        });
    }

    /// A list of strings, a single string counting as one, if given.
    public Optional<List<String>> strings(String key) {
        return get(key).map(v -> switch (v) {
            case List<?> l -> l.stream().map(String::valueOf).toList();
            case String s -> List.of(s);
            default -> throw bad(key, "takes a list");
        });
    }

    /// A map of strings, if given.
    public Optional<Map<String, String>> table(String key) {
        return get(key).map(v -> {
            if (!(v instanceof Map<?, ?> m)) throw bad(key, "takes a map");
            var out = new LinkedHashMap<String, String>();
            m.forEach((k, x) -> out.put(String.valueOf(k), String.valueOf(x)));
            return out;
        });
    }

    /// Every key, read or not, for factories that take free-form keys.
    public Map<String, Object> all() {
        read.addAll(keys.keySet());
        return new LinkedHashMap<>(keys);
    }

    /// Fails on the first key left unread.
    public void done() {
        for (var k : keys.keySet())
            if (!read.contains(k)) throw new SerlogException.Config(what + ": unknown key `" + k + "`");
    }
}
