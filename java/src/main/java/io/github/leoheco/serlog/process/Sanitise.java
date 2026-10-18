package io.github.leoheco.serlog.process;

import io.github.leoheco.serlog.Field;
import io.github.leoheco.serlog.LogRecord;
import java.util.ArrayList;
import java.util.Map;
import java.util.Optional;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/// Escapes line breaks and control characters in the message and string fields, against log forging.
public record Sanitise() implements Processor {
    private static final Pattern CONTROL = Pattern.compile("[\\x00-\\x1f\\x7f]");

    private static String clean(String s) {
        return CONTROL.matcher(s)
                .replaceAll(m -> Matcher.quoteReplacement(
                        switch (m.group().charAt(0)) {
                            case '\n' -> "\\n";
                            case '\r' -> "\\r";
                            case '\t' -> "\\t";
                            default -> "\\u{%04x}".formatted((int) m.group().charAt(0));
                        }));
    }

    @Override
    public LogRecord process(LogRecord rec) {
        var msg = clean(rec.msg());
        var same = msg.equals(rec.msg());
        var fields = new ArrayList<Field>(rec.fields().size());
        for (var f : rec.fields()) {
            if (f.value() instanceof String s) {
                var c = clean(s);
                same &= c.equals(s);
                fields.add(c.equals(s) ? f : new Field(f.key(), c));
            } else fields.add(f);
        }
        return same ? rec : rec.withMsg(msg).withFields(fields);
    }

    @Override
    public Optional<Map<String, Object>> options() {
        return Optional.of(Map.of("sanitise", true));
    }
}
