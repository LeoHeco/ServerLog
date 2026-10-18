package io.github.leoheco.serlog;

import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.regex.Pattern;

/// A severity: six built-in levels from 10 to 60, plus any defined at run time. Levels compare by value only.
///
/// @param value a number from 0 to 255
public record Level(int value) implements Comparable<Level> {
    public static final Level TRACE = new Level(10);
    public static final Level DEBUG = new Level(20);
    public static final Level INFO = new Level(30);
    public static final Level WARN = new Level(40);
    public static final Level ERROR = new Level(50);
    public static final Level FATAL = new Level(60);

    private record Info(String name, Colour colour) {}

    private static final Map<Integer, Info> BUILT_IN = Map.of(
            10, new Info("trace", Colour.CYAN),
            20, new Info("debug", Colour.MAGENTA),
            30, new Info("info", Colour.GREEN),
            40, new Info("warn", Colour.YELLOW),
            50, new Info("error", Colour.RED),
            60, new Info("fatal", Colour.ALARM));
    private static final Map<String, Integer> ALIASES = Map.of("warning", 40, "critical", 60);
    private static final Map<Integer, Info> CUSTOM = new ConcurrentHashMap<>();
    private static final Pattern NAME = Pattern.compile("[a-z0-9_]+");

    public Level {
        if (value < 0 || value > 255) throw new IllegalArgumentException("a level is 0 to 255, not " + value);
    }

    /// The level with this value.
    public static Level of(int value) {
        return new Level(value);
    }

    /// A level from a name in any case, `warning` and `critical` included, or from digits.
    public static Level parse(String name) {
        var s = name.strip().toLowerCase(Locale.ROOT);
        if (!s.isEmpty() && s.chars().allMatch(Character::isDigit)) return new Level(Integer.parseInt(s));
        var alias = ALIASES.get(s);
        if (alias != null) return new Level(alias);
        for (var table : java.util.List.of(BUILT_IN, CUSTOM))
            for (var e : table.entrySet()) if (e.getValue().name().equals(s)) return new Level(e.getKey());
        throw new IllegalArgumentException("unknown level `" + name
                + "`: expected trace, debug, info, warn, error, fatal, a defined level or a number");
    }

    /// A level from a [Level], a number or a name.
    public static Level from(Object level) {
        return switch (level) {
            case Level l -> l;
            case Number n -> new Level(n.intValue());
            case String s -> parse(s);
            default -> throw new IllegalArgumentException("not a level: " + level);
        };
    }

    /// Defines a level, or renames the one with that value.
    public static Level define(String name, int value, Colour colour) {
        if (!NAME.matcher(name).matches()
                || name.equals("off")
                || ALIASES.containsKey(name)
                || BUILT_IN.values().stream().anyMatch(i -> i.name().equals(name)))
            throw new IllegalArgumentException("bad level name `" + name + "`");
        var level = new Level(value);
        CUSTOM.values().removeIf(i -> i.name().equals(name));
        CUSTOM.put(value, new Info(name, colour));
        return level;
    }

    /// Every level defined at run time, by value, with its name and colour.
    public static Map<Level, Map.Entry<String, Colour>> defined() {
        var out = new java.util.TreeMap<Level, Map.Entry<String, Colour>>();
        CUSTOM.forEach((v, i) -> out.put(new Level(v), Map.entry(i.name(), i.colour())));
        return out;
    }

    private Info info() {
        var i = BUILT_IN.get(value);
        return i != null ? i : CUSTOM.get(value);
    }

    /// The lower-case name used in config files, filters and labels; `custom` for an unnamed value.
    public String id() {
        var i = info();
        return i == null ? "custom" : i.name();
    }

    /// Its colour in a terminal.
    public Colour colour() {
        var i = info();
        return i == null ? Colour.PLAIN : i.colour();
    }

    /// How a profile prints it, such as `WARNING` for Python's.
    public String label(Compat profile) {
        if (profile == Compat.PYTHON && value == 40) return "WARNING";
        if (profile == Compat.PYTHON && value == 60) return "CRITICAL";
        if (profile == Compat.SLOG && value == 10) return "DEBUG-4";
        if (profile == Compat.SLOG && value == 60) return "ERROR+4";
        return id().toUpperCase(Locale.ROOT);
    }

    /// The syslog severity, for GELF.
    public int syslog() {
        return value <= 20 ? 7 : value <= 30 ? 6 : value <= 40 ? 4 : value <= 50 ? 3 : 2;
    }

    /// Whether this is at least `other`.
    public boolean atLeast(Level other) {
        return value >= other.value;
    }

    @Override
    public int compareTo(Level o) {
        return Integer.compare(value, o.value);
    }

    @Override
    public String toString() {
        return id();
    }
}
