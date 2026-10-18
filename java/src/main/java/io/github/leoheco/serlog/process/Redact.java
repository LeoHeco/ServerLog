package io.github.leoheco.serlog.process;

import io.github.leoheco.serlog.Field;
import io.github.leoheco.serlog.LogRecord;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeSet;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/// Masks fields by name at any depth or by dotted path, and parts of the message by regex.
///
/// @param keys field names to mask wherever they appear
/// @param paths dotted paths to mask, such as `req.headers.authorization`
/// @param patterns regexes to mask in the message
/// @param mask what goes in their place, `***` by default
public record Redact(Set<String> keys, List<String> paths, List<Pattern> patterns, String mask) implements Processor {
    public Redact {
        keys = Set.copyOf(keys);
        paths = List.copyOf(paths);
        patterns = List.copyOf(patterns);
    }

    /// Masks fields with these names.
    public static Redact keys(String... keys) {
        return new Redact(Set.of(keys), List.of(), List.of(), "***");
    }

    /// A copy that also masks these dotted paths.
    public Redact paths(String... more) {
        var p = new ArrayList<>(paths);
        p.addAll(List.of(more));
        return new Redact(keys, p, patterns, mask);
    }

    /// A copy that also masks these regexes in the message.
    public Redact patterns(String... more) {
        var p = new ArrayList<>(patterns);
        for (var s : more) p.add(Pattern.compile(s));
        return new Redact(keys, paths, p, mask);
    }

    /// A copy with another mask.
    public Redact mask(String mask) {
        return new Redact(keys, paths, patterns, mask);
    }

    private Object scrub(String path, String key, Object v) {
        if (keys.contains(key) || paths.contains(path)) return mask;
        if (!(v instanceof Map<?, ?> m)) return v;
        var out = new LinkedHashMap<Object, Object>();
        var same = true;
        for (var e : m.entrySet()) {
            var k = String.valueOf(e.getKey());
            var x = scrub(path + "." + k, k, e.getValue());
            same &= x == e.getValue();
            out.put(e.getKey(), x);
        }
        return same ? v : out;
    }

    @Override
    public LogRecord process(LogRecord rec) {
        var msg = rec.msg();
        for (var p : patterns) msg = p.matcher(msg).replaceAll(Matcher.quoteReplacement(mask));
        var fields = new ArrayList<Field>(rec.fields().size());
        var same = msg.equals(rec.msg());
        for (var f : rec.fields()) {
            var v = scrub(f.key(), f.key(), f.value());
            same &= v == f.value();
            fields.add(v == f.value() ? f : new Field(f.key(), v));
        }
        return same ? rec : rec.withMsg(msg).withFields(fields);
    }

    @Override
    public Optional<Map<String, Object>> options() {
        var o = new LinkedHashMap<String, Object>();
        if (!keys.isEmpty()) o.put("keys", List.copyOf(new TreeSet<>(keys)));
        if (!paths.isEmpty()) o.put("paths", paths);
        if (!patterns.isEmpty())
            o.put("patterns", patterns.stream().map(Pattern::pattern).toList());
        if (!mask.equals("***")) o.put("mask", mask);
        return Optional.of(Map.of("redact", o));
    }
}
