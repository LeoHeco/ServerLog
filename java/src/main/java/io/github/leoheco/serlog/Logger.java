package io.github.leoheco.serlog;

import io.github.leoheco.serlog.internal.Hub;
import io.github.leoheco.serlog.internal.Make;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.function.Supplier;

/// Makes records for one target, with fields of its own. Messages take `{}` placeholders; a trailing throwable
/// becomes the `error` field, and a [Supplier] argument is only called when the record passes.
///
/// ```java
/// var log = Serlog.get("app.sync").with("mirror", "debian");
/// log.info("synced {} files", 42);
/// log.atWarn().field("tries", 3).tag("sync").log("gave up");
/// ```
public final class Logger {
    private final String target;
    private final List<Field> fields;

    Logger(String target, List<Field> fields) {
        this.target = target;
        this.fields = List.copyOf(fields);
    }

    /// Its target.
    public String target() {
        return target;
    }

    /// A logger with one more field.
    public Logger with(String key, Object value) {
        var f = new ArrayList<>(fields);
        f.add(new Field(key, value));
        return new Logger(target, f);
    }

    /// A logger with more fields.
    public Logger with(Map<String, ?> more) {
        var f = new ArrayList<>(fields);
        more.forEach((k, v) -> f.add(new Field(k, v)));
        return new Logger(target, f);
    }

    /// The same fields under another target.
    public Logger named(String target) {
        return new Logger(target, fields);
    }

    /// Whether a record at `level` could pass.
    public boolean enabled(Level level) {
        return Hub.enabled(level);
    }

    /// Logs at `level`.
    public void log(Level level, String msg, Object... args) {
        if (Hub.enabled(level)) Hub.emit(Make.record(level, target, fields, List.of(), msg, args));
    }

    public void trace(String msg, Object... args) {
        log(Level.TRACE, msg, args);
    }

    public void debug(String msg, Object... args) {
        log(Level.DEBUG, msg, args);
    }

    public void info(String msg, Object... args) {
        log(Level.INFO, msg, args);
    }

    public void warn(String msg, Object... args) {
        log(Level.WARN, msg, args);
    }

    public void error(String msg, Object... args) {
        log(Level.ERROR, msg, args);
    }

    public void fatal(String msg, Object... args) {
        log(Level.FATAL, msg, args);
    }

    /// Logs `e` at error as the `error` field.
    public void exception(Throwable e, String msg, Object... args) {
        if (Hub.enabled(Level.ERROR)) at(Level.ERROR).error(e).log(msg, args);
    }

    /// A record built step by step at `level`; nothing is made when the level is off.
    public Event at(Level level) {
        return Hub.enabled(level) ? new Event(this, level) : Event.OFF;
    }

    public Event atTrace() {
        return at(Level.TRACE);
    }

    public Event atDebug() {
        return at(Level.DEBUG);
    }

    public Event atInfo() {
        return at(Level.INFO);
    }

    public Event atWarn() {
        return at(Level.WARN);
    }

    public Event atError() {
        return at(Level.ERROR);
    }

    public Event atFatal() {
        return at(Level.FATAL);
    }

    /// A record built step by step: fields, tags and an error, then [#log].
    public static final class Event {
        static final Event OFF = new Event(null, null);

        private final Logger logger;
        private final Level level;
        private final List<Field> fields;
        private final List<String> tags;

        private Event(Logger logger, Level level) {
            this.logger = logger;
            this.level = level;
            this.fields = logger == null ? List.of() : new ArrayList<>(logger.fields);
            this.tags = logger == null ? List.of() : new ArrayList<>();
        }

        /// Adds a field.
        public Event field(String key, Object value) {
            if (logger != null) fields.add(new Field(key, value));
            return this;
        }

        /// Adds a field worked out only now that the record passes.
        public Event field(String key, Supplier<?> value) {
            if (logger != null) fields.add(new Field(key, value.get()));
            return this;
        }

        /// Adds fields.
        public Event fields(Map<String, ?> more) {
            if (logger != null) more.forEach((k, v) -> fields.add(new Field(k, v)));
            return this;
        }

        /// Adds tags.
        public Event tag(String... more) {
            if (logger != null) tags.addAll(List.of(more));
            return this;
        }

        /// Sets the `error` field.
        public Event error(Throwable e) {
            return field("error", e);
        }

        /// Makes the record.
        public void log(String msg, Object... args) {
            if (logger != null) Hub.emit(Make.record(level, logger.target, fields, tags, msg, args));
        }
    }
}
