package io.github.leoheco.serlog;

import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.layout.JsonLayout;
import io.github.leoheco.serlog.layout.Layout;
import io.github.leoheco.serlog.process.Processor;
import java.time.Duration;
import java.util.List;
import java.util.Map;

/// The slot settings every sink may carry alongside its own: the keys of a config entry, set in code. Values are
/// checked when the sink is registered.
///
/// @param <S> the type that carries them, returned by every setter
public interface SlotKeys<S extends SlotKeys<S>> {
    /// Sets a slot key by its config name, such as `alarm_only`.
    S set(String key, Object value);

    /// Its name; the kind and a number by default.
    default S name(String name) {
        return set("name", name);
    }

    /// Lets `level` and above through.
    default S level(Level level) {
        return set("level", level);
    }

    /// Lets through what the directives allow, such as `info,app.db=debug`.
    default S filter(String filter) {
        return set("filter", filter);
    }

    /// Its own processors, run after the global ones.
    default S process(Processor... steps) {
        return set("process", List.of(steps));
    }

    /// Its layout.
    default S layout(Layout layout) {
        return set("layout", layout);
    }

    /// `text`, `logfmt`, `json` or `pretty`.
    default S format(String format) {
        return set("format", format);
    }

    /// A pattern or preset name.
    default S pattern(String pattern) {
        return set("pattern", pattern);
    }

    /// A JSON schema.
    default S schema(JsonLayout.Schema schema) {
        return set("schema", schema);
    }

    /// New names for JSON keys.
    default S keys(Map<String, String> keys) {
        return set("keys", keys);
    }

    /// Whether JSON levels are names or numbers.
    default S levels(JsonLayout.Levels levels) {
        return set("levels", levels);
    }

    /// The key JSON fields go under.
    default S nest(String key) {
        return set("nest", key);
    }

    /// When to colour.
    default S colour(Colour.When when) {
        return set("colour", when);
    }

    /// Other colours for some levels.
    default S colours(Theme theme) {
        return set("colours", theme);
    }

    /// Whether only alarms reach it.
    default S alarmOnly(boolean only) {
        return set("alarm_only", only);
    }

    /// What happens when it fails: an alarm.
    default S onFail(Alarm alarm) {
        return set("on_fail", alarm);
    }

    /// What happens when it fails: `panic`, `ignore`, `default` or an alarm's name.
    default S onFail(String policy) {
        return set("on_fail", policy);
    }

    /// A queue of this many records; 0 writes directly.
    default S queue(int size) {
        return set("queue", size);
    }

    /// How many queued records are written at once.
    default S batch(int batch) {
        return set("batch", batch);
    }

    /// How long the first queued record waits for others.
    default S wait(Duration wait) {
        return set("wait", wait);
    }

    /// What a full queue does.
    default S full(Queued.Full full) {
        return set("full", full);
    }

    /// How many times a failed write is tried again.
    default S retries(int retries) {
        return set("retries", retries);
    }

    /// How long a failed sink rests before it is tried again.
    default S probe(Duration probe) {
        return set("probe", probe);
    }
}
