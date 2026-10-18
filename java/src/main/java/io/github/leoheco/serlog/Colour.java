package io.github.leoheco.serlog;

import java.util.Locale;

/// A terminal colour.
public enum Colour {
    PLAIN(""),
    RED("\u001b[1;31m"),
    GREEN("\u001b[32m"),
    YELLOW("\u001b[33m"),
    BLUE("\u001b[34m"),
    MAGENTA("\u001b[35m"),
    CYAN("\u001b[36m"),
    WHITE("\u001b[37m"),
    GREY("\u001b[90m"),
    ALARM("\u001b[1;37;41m");

    private final String ansi;

    Colour(String ansi) {
        this.ansi = ansi;
    }

    /// A colour by its config name; `gray` means `grey`.
    public static Colour parse(String name) {
        var n = name.strip().toLowerCase(Locale.ROOT);
        try {
            return valueOf((n.equals("gray") ? "grey" : n).toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException e) {
            throw new IllegalArgumentException("unknown colour `" + name + "`", e);
        }
    }

    /// `text` in this colour when `on`.
    public String paint(boolean on, String text) {
        return on && this != PLAIN ? ansi + text + "\u001b[0m" : text;
    }

    /// The config name.
    public String id() {
        return name().toLowerCase(Locale.ROOT);
    }

    /// When to colour.
    public enum When {
        AUTO,
        ALWAYS,
        NEVER;

        /// A setting by its config name.
        public static When parse(String name) {
            return valueOf(name.strip().toUpperCase(Locale.ROOT));
        }

        /// Decides for a sink that is, or is not, a terminal; `auto` honours `NO_COLOR` and `FORCE_COLOR`.
        public boolean resolve(boolean tty) {
            return switch (this) {
                case ALWAYS -> true;
                case NEVER -> false;
                case AUTO -> {
                    var force = System.getenv("FORCE_COLOR");
                    var none = System.getenv("NO_COLOR");
                    yield force != null && !force.isEmpty() || tty && (none == null || none.isEmpty());
                }
            };
        }

        /// The config name.
        public String id() {
            return name().toLowerCase(Locale.ROOT);
        }
    }
}
