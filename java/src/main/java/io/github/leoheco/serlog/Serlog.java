package io.github.leoheco.serlog;

import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.internal.AlarmTask;
import io.github.leoheco.serlog.internal.Hub;
import io.github.leoheco.serlog.internal.Make;
import io.github.leoheco.serlog.internal.Start;
import io.github.leoheco.serlog.process.Processor;
import io.github.leoheco.serlog.process.Redact;
import java.time.Duration;
import java.util.List;
import java.util.Map;
import java.util.function.Supplier;

/// A modern, high-performance logging framework: any number of sinks, each configured on its own.
///
/// ```java
/// Serlog.info("synced {} files", 42);
/// Serlog.builder().app("mirror").sink(new ConsoleSink()).sink(new FileSink("logs/app.log").keep(7)).init();
/// ```
///
/// With no set-up at all it prints `info` and above to standard output, like most loggers.
public final class Serlog {
    private Serlog() {}

    /// Starts from the file `SERLOG_CONFIG` names, or the first of `serlog.yaml`, `serlog.yml`, `serlog.json` and
    /// `serlog.toml` in the working directory; failing that it starts a console sink. Runs once.
    public static void init() {
        Start.init();
    }

    /// Starts from a config tree, such as a parsed file or a section of another program's config. Runs once.
    public static void init(Map<String, ?> settings) {
        Start.start(settings);
    }

    /// Settings built in code, started with [Builder#init].
    public static Builder builder() {
        return new Builder();
    }

    /// Whether serlog was started.
    public static boolean started() {
        return Start.started();
    }

    /// A logger for `target`.
    public static Logger get(String target) {
        return new Logger(target, List.of());
    }

    /// A logger named after a class.
    public static Logger get(Class<?> type) {
        return get(type.getName());
    }

    /// A logger for `target` with fields of its own.
    public static Logger get(String target, Map<String, ?> fields) {
        return get(target).with(fields);
    }

    private static Logger root() {
        var app = Hub.meta().app();
        return get(app.isEmpty() ? "main" : app);
    }

    /// Logs at `level` through the app's logger.
    public static void log(Level level, String msg, Object... args) {
        if (Hub.enabled(level)) root().log(level, msg, args);
    }

    public static void trace(String msg, Object... args) {
        log(Level.TRACE, msg, args);
    }

    public static void debug(String msg, Object... args) {
        log(Level.DEBUG, msg, args);
    }

    public static void info(String msg, Object... args) {
        log(Level.INFO, msg, args);
    }

    public static void warn(String msg, Object... args) {
        log(Level.WARN, msg, args);
    }

    public static void error(String msg, Object... args) {
        log(Level.ERROR, msg, args);
    }

    public static void fatal(String msg, Object... args) {
        log(Level.FATAL, msg, args);
    }

    /// Logs `e` at error through the app's logger.
    public static void exception(Throwable e, String msg, Object... args) {
        if (Hub.enabled(Level.ERROR)) root().exception(e, msg, args);
    }

    /// A record built step by step through the app's logger.
    public static Logger.Event at(Level level) {
        return root().at(level);
    }

    /// A value worked out only when the record passes: `Serlog.debug("state {}", Serlog.lazy(this::dump))`.
    public static Supplier<?> lazy(Supplier<?> fn) {
        return fn;
    }

    /// Whether a record at `level` could pass the filter for some target.
    public static boolean enabled(Level level) {
        return Hub.enabled(level);
    }

    /// Sends a record made elsewhere through the filter, the scope and the processors.
    public static void emit(LogRecord rec) {
        Hub.emit(Make.caller(rec));
    }

    /// Runs `fn` with `fields` added to every record made inside it.
    public static void bind(Map<String, ?> fields, Runnable fn) {
        Hub.bind(fields, () -> {
            fn.run();
            return null;
        });
    }

    /// Runs `fn` with `fields` added to every record made inside it, and returns what it returns.
    public static <T, X extends Throwable> T bind(Map<String, ?> fields, ScopedValue.CallableOp<T, X> fn) throws X {
        return Hub.bind(fields, fn);
    }

    /// Registers a sink with the slot settings it carries; returns the name it got.
    public static String add(Sink sink) {
        return Hub.add(Slot.of(sink));
    }

    /// Registers a sink with slot settings; returns the name it got.
    public static String add(Slot slot) {
        return Hub.add(slot);
    }

    /// Takes a slot out; what its queue holds is still written. `false` when no slot has that name.
    public static boolean remove(String name) {
        return Hub.remove(name);
    }

    /// Every slot's state.
    public static List<Status> status() {
        return Hub.status();
    }

    /// Incidents a full alarm queue dropped.
    public static long alarmsDropped() {
        return AlarmTask.dropped();
    }

    /// Waits up to five seconds for every queue and the alarm route to drain.
    public static void flush() {
        flush(Duration.ofSeconds(5));
    }

    /// Waits up to `within` for every queue and the alarm route to drain.
    public static void flush(Duration within) {
        Hub.flush(within);
    }

    /// The global filter in force.
    public static Filter filter() {
        return Hub.filter();
    }

    /// Replaces the global filter; takes effect at once.
    public static void setFilter(String spec) {
        Hub.setFilter(new Filter(spec));
    }

    /// Lets through `level` and above for every target.
    public static void setLevel(Level level) {
        Hub.setFilter(new Filter(level));
    }

    /// Replaces the global processor chain.
    public static void setProcess(List<? extends Processor> chain) {
        Hub.setSteps(List.copyOf(chain));
    }

    /// Adds a step to the end of the global processor chain.
    public static void process(Processor step) {
        Hub.process(step);
    }

    /// Masks fields with these names, wherever they appear.
    public static void redact(String... keys) {
        Hub.process(Redact.keys(keys));
    }

    /// Replaces the default alarm.
    public static void setAlarm(Alarm alarm) {
        Hub.setAlarm(alarm);
    }

    /// The compat profile in force.
    public static Compat compat() {
        return Hub.compat();
    }

    /// The identity in force.
    public static Meta meta() {
        return Hub.meta();
    }

    /// Logs uncaught exceptions at `fatal`, then hands them to the handler that was there before.
    public static void catchUncaught() {
        var before = Thread.getDefaultUncaughtExceptionHandler();
        Thread.setDefaultUncaughtExceptionHandler((t, e) -> {
            if (Hub.enabled(Level.FATAL))
                Hub.emit(LogRecord.of(
                        Level.FATAL,
                        "uncaught",
                        "uncaught in " + t.getName() + ": " + SerlogException.message(e),
                        List.of(Field.of("error", e))));
            Hub.flush(Duration.ofSeconds(1));
            if (before != null) before.uncaughtException(t, e);
            else {
                System.err.print("Exception in thread \"" + t.getName() + "\" ");
                e.printStackTrace();
            }
        });
    }
}
