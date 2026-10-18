package io.github.leoheco.serlog;

import java.util.List;
import java.util.Locale;

/// A profile that makes serlog behave like another logger: its pattern, console stream, default level and the
/// environment variables it reads for the filter. `log4j` is another name for `logback`.
public enum Compat {
    SERLOG("{time} {level:<5} {target}: {msg}{fields}", Output.STDOUT, Level.INFO, List.of("SERLOG_FILTER")),
    TRACING("{time:rfc3339} {level:>5} {spans}{target}: {msg}{fields}", Output.STDOUT, Level.INFO, List.of("RUST_LOG")),
    ENV_LOGGER("[{time:utc} {level:<5} {target}] {msg}", Output.STDERR, Level.ERROR, List.of("RUST_LOG")),
    PYTHON("{level}:{target}:{msg}", Output.STDERR, Level.WARN, List.of()),
    LOGBACK("{time:%H:%M:%S%.3f} [{thread}] {level:<5} {target:~36} - {msg}", Output.STDOUT, Level.DEBUG, List.of()),
    SLOG(
            "time={time:%Y-%m-%dT%H:%M:%S%.3f%:z} level={level} msg={msg:q}{fields:q}",
            Output.STDOUT, Level.INFO, List.of()),
    PINO("[{time:%H:%M:%S%.3f}] {level} ({pid}): {msg}{fields}", Output.STDOUT, Level.INFO, List.of());

    private final String pattern;
    private final Output output;
    private final Level level;
    private final List<String> env;

    Compat(String pattern, Output output, Level level, List<String> env) {
        this.pattern = pattern;
        this.output = output;
        this.level = level;
        this.env = env;
    }

    /// A profile by its config name.
    public static Compat parse(String name) {
        var n = name.strip().toLowerCase(Locale.ROOT);
        for (var c : values()) if (c.id().equals(n.equals("log4j") ? "logback" : n)) return c;
        throw new IllegalArgumentException("unknown compat profile `" + name + "`");
    }

    /// The profile a preset name such as `tracing` stands for, or `null`.
    public static Compat preset(String name) {
        try {
            return parse(name);
        } catch (IllegalArgumentException e) {
            return null;
        }
    }

    /// The config name.
    public String id() {
        return name().toLowerCase(Locale.ROOT);
    }

    /// The pattern that reproduces this logger's default output.
    public String pattern() {
        return pattern;
    }

    /// Where its console writes.
    public Output output() {
        return output;
    }

    /// Its default level.
    public Level level() {
        return level;
    }

    /// The environment variables it reads for the filter.
    public List<String> env() {
        return env;
    }

    /// Where a console writes.
    public enum Output {
        STDOUT,
        STDERR,
        /// `error` and above to standard error, the rest to standard output.
        AUTO;

        /// A stream by its config name.
        public static Output parse(String name) {
            return valueOf(name.strip().toUpperCase(Locale.ROOT));
        }

        /// The config name.
        public String id() {
            return name().toLowerCase(Locale.ROOT);
        }
    }
}
