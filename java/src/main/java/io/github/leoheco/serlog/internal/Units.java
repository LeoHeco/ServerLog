package io.github.leoheco.serlog.internal;

import java.time.Duration;
import java.util.Locale;
import java.util.Map;
import java.util.regex.Pattern;

/// Durations and sizes as config files write them: `250ms`, `5s`, `2m`, `1h`, `1d`; `500KB`, `64MiB`.
public final class Units {
    private static final Map<String, Double> TIME =
            Map.of("ms", 0.001, "", 1.0, "s", 1.0, "m", 60.0, "h", 3600.0, "d", 86400.0);
    private static final Map<String, Long> SIZE = Map.ofEntries(
            Map.entry("", 1L),
            Map.entry("b", 1L),
            Map.entry("k", 1_000L),
            Map.entry("kb", 1_000L),
            Map.entry("kib", 1L << 10),
            Map.entry("m", 1_000_000L),
            Map.entry("mb", 1_000_000L),
            Map.entry("mib", 1L << 20),
            Map.entry("g", 1_000_000_000L),
            Map.entry("gb", 1_000_000_000L),
            Map.entry("gib", 1L << 30));
    private static final Pattern NUMBER = Pattern.compile("\\s*(\\d+(?:\\.\\d+)?)\\s*([a-zA-Z]*)\\s*");

    private Units() {}

    /// A duration in seconds: a number, a [Duration], or text with `ms`, `s`, `m`, `h` or `d`.
    public static double seconds(Object v) {
        return switch (v) {
            case Duration d -> d.toNanos() / 1e9;
            case Number n
            when n.doubleValue() < 0 -> throw new IllegalArgumentException("a duration cannot be negative: " + n);
            case Number n -> n.doubleValue();
            case String s -> {
                var m = NUMBER.matcher(s);
                var unit = m.matches() ? TIME.get(m.group(2)) : null;
                if (unit == null) throw new IllegalArgumentException("bad duration `" + s + "`");
                yield Double.parseDouble(m.group(1)) * unit;
            }
            default -> throw new IllegalArgumentException("bad duration `" + v + "`");
        };
    }

    /// A duration from a number of seconds, a [Duration] or text.
    public static Duration duration(Object v) {
        return v instanceof Duration d ? d : Duration.ofNanos(Math.round(seconds(v) * 1e9));
    }

    /// A byte count: a number, or text such as `500KB` or `64MiB`.
    public static long size(Object v) {
        return switch (v) {
            case Number n -> n.longValue();
            case String s -> {
                var m = NUMBER.matcher(s);
                var unit = m.matches() ? SIZE.get(m.group(2).toLowerCase(Locale.ROOT)) : null;
                if (unit == null) throw new IllegalArgumentException("bad size `" + s + "`");
                yield (long) Math.floor(Double.parseDouble(m.group(1)) * unit);
            }
            default -> throw new IllegalArgumentException("bad size `" + v + "`");
        };
    }

    /// The shortest way to write a duration that reads back the same.
    public static String showTime(double secs) {
        var ms = Math.round(secs * 1000);
        for (var u : new Object[][] {{86_400_000L, "d"}, {3_600_000L, "h"}, {60_000L, "m"}, {1000L, "s"}}) {
            var n = (long) u[0];
            if (ms >= n && ms % n == 0) return ms / n + (String) u[1];
        }
        return ms + "ms";
    }

    /// The shortest way to write a duration that reads back the same.
    public static String showTime(Duration d) {
        return showTime(d.toNanos() / 1e9);
    }

    /// The shortest way to write a byte count that reads back the same: a number or text.
    public static Object showSize(long n) {
        for (var u : new Object[][] {{1L << 30, "GiB"}, {1L << 20, "MiB"}, {1L << 10, "KiB"}}) {
            var m = (long) u[0];
            if (n >= m && n % m == 0) return n / m + (String) u[1];
        }
        return n;
    }
}
