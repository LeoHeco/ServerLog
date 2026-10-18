package io.github.leoheco.serlog.layout;

import java.time.Instant;
import java.time.ZoneId;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.time.temporal.IsoFields;
import java.util.regex.Pattern;

/// How a record's time is written.
public final class Clock {
    private static final long START = System.nanoTime();
    private static final Pattern DIRECTIVE = Pattern.compile("%(:z|\\.\\d+f|.)");

    private Clock() {}

    /// Milliseconds since serlog was loaded.
    public static long elapsed() {
        return (System.nanoTime() - START) / 1_000_000;
    }

    private static String pad(long n, int w) {
        var s = Long.toString(n);
        return s.length() >= w ? s : "0".repeat(w - s.length()) + s;
    }

    private static String offset(ZonedDateTime t, boolean colon) {
        var secs = t.getOffset().getTotalSeconds();
        var a = Math.abs(secs) / 60;
        return (secs < 0 ? "-" : "+") + pad(a / 60, 2) + (colon ? ":" : "") + pad(a % 60, 2);
    }

    private static String fraction(int nanos, int digits) {
        return "." + pad(nanos, 9).substring(0, Math.min(digits, 9));
    }

    /// strftime in local time for `%Y %y %m %d %H %M %S %j %F %T %z %:z %G %V %%`, plus `%.3f`, `%.6f` and `%.9f`
    /// for fractions of a second; other directives pass through.
    public static String strftime(Instant time, String fmt) {
        return strftime(time.atZone(ZoneId.systemDefault()), fmt);
    }

    /// strftime in the time's own zone.
    public static String strftime(ZonedDateTime t, String fmt) {
        return DIRECTIVE
                .matcher(fmt)
                .replaceAll(m -> java.util.regex.Matcher.quoteReplacement(
                        switch (m.group(1)) {
                            case "Y" -> String.valueOf(t.getYear());
                            case "y" -> pad(t.getYear() % 100, 2);
                            case "m" -> pad(t.getMonthValue(), 2);
                            case "d" -> pad(t.getDayOfMonth(), 2);
                            case "H" -> pad(t.getHour(), 2);
                            case "M" -> pad(t.getMinute(), 2);
                            case "S" -> pad(t.getSecond(), 2);
                            case "j" -> pad(t.getDayOfYear(), 3);
                            case "F" -> strftime(t, "%Y-%m-%d");
                            case "T" -> strftime(t, "%H:%M:%S");
                            case "z" -> offset(t, false);
                            case ":z" -> offset(t, true);
                            case "G" -> String.valueOf(t.get(IsoFields.WEEK_BASED_YEAR));
                            case "V" -> pad(t.get(IsoFields.WEEK_OF_WEEK_BASED_YEAR), 2);
                            case "%" -> "%";
                            case String d
                            when d.startsWith(".") ->
                                fraction(t.getNano(), Integer.parseInt(d.substring(1, d.length() - 1)));
                            default -> m.group();
                        }));
    }

    /// UTC RFC 3339 with `digits` decimals: `2023-11-14T22:13:20.000Z`.
    public static String iso(Instant time, int digits) {
        var t = time.atZone(ZoneOffset.UTC);
        var base = strftime(t, "%Y-%m-%dT%H:%M:%S");
        return digits > 0 ? base + fraction(t.getNano(), digits) + "Z" : base + "Z";
    }

    /// `time` in the format a `{time}` token asks for.
    public static String write(String spec, Instant time) {
        return switch (spec) {
            case "", "local" -> strftime(time, "%Y-%m-%d %H:%M:%S%.3f");
            case "utc" -> iso(time, 0);
            case "rfc3339" -> strftime(time, "%Y-%m-%dT%H:%M:%S%.6f%:z");
            case "unix" -> String.valueOf(time.getEpochSecond());
            case "unix_ms" -> String.valueOf(time.toEpochMilli());
            case "unix_ns" -> String.valueOf(time.getEpochSecond() * 1_000_000_000L + time.getNano());
            default -> strftime(time, spec);
        };
    }
}
