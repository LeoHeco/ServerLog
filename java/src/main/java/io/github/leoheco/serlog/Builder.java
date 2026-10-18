package io.github.leoheco.serlog;

import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.internal.Start;
import io.github.leoheco.serlog.process.Processor;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/// Start-up settings in code; every method sets the config key of the same name.
///
/// ```java
/// Serlog.builder()
///         .app("mirror")
///         .filter("info,app.db=debug")
///         .sink(new ConsoleSink())
///         .sink(new FileSink("logs/app.log").rotate(FileSink.Rotate.DAY).keep(7).name("app"))
///         .alarm("default", Tiers.of("oncall", "stderr"))
///         .init();
/// ```
public final class Builder {
    private final Map<String, Object> cfg = new LinkedHashMap<>();
    private final List<Object> sinks = new ArrayList<>();
    private final List<Object> process = new ArrayList<>();
    private final List<String> redact = new ArrayList<>();
    private final Map<String, Object> alarms = new LinkedHashMap<>();
    private final Map<String, Object> levels = new LinkedHashMap<>();

    Builder() {}

    public Builder app(String app) {
        cfg.put("app", app);
        return this;
    }

    public Builder env(String env) {
        cfg.put("env", env);
        return this;
    }

    public Builder instance(String instance) {
        cfg.put("instance", instance);
        return this;
    }

    /// Takes on another logger's pattern, stream, level and filter variables.
    public Builder compat(Compat profile) {
        cfg.put("compat", profile);
        return this;
    }

    /// The default level.
    public Builder level(Level level) {
        cfg.put("level", level);
        return this;
    }

    /// Filter directives, such as `info,app.db=debug`.
    public Builder filter(String spec) {
        cfg.put("filter", spec);
        return this;
    }

    /// An environment variable that overrides the filter.
    public Builder filterEnv(String name) {
        cfg.put("filter_env", name);
        return this;
    }

    /// Defines a level.
    public Builder level(String name, int value, Colour colour) {
        levels.put(name, Map.of("value", value, "colour", colour.id()));
        return this;
    }

    /// Adds a step to the global chain.
    public Builder process(Processor step) {
        process.add(step);
        return this;
    }

    /// Masks fields with these names.
    public Builder redact(String... keys) {
        redact.addAll(List.of(keys));
        return this;
    }

    /// Adds a sink with the slot settings it carries.
    public Builder sink(Sink sink) {
        sinks.add(sink);
        return this;
    }

    /// Adds a sink with slot settings.
    public Builder sink(Slot slot) {
        sinks.add(slot);
        return this;
    }

    /// Adds a sink from a config entry.
    public Builder sink(Map<String, ?> entry) {
        sinks.add(entry);
        return this;
    }

    /// Names an alarm for `on_fail`; `default` replaces the default alarm.
    public Builder alarm(String name, Alarm alarm) {
        alarms.put(name, alarm);
        return this;
    }

    /// The settings as a config tree.
    public Map<String, Object> tree() {
        var out = new LinkedHashMap<>(cfg);
        if (!levels.isEmpty()) out.put("levels", levels);
        if (!process.isEmpty()) out.put("process", List.copyOf(process));
        if (!redact.isEmpty()) out.put("redact", List.copyOf(redact));
        if (!sinks.isEmpty()) out.put("sinks", List.copyOf(sinks));
        if (!alarms.isEmpty()) out.put("alarms", alarms);
        return out;
    }

    /// Starts serlog; checks everything first and changes nothing on error. Runs once.
    public void init() {
        Start.start(tree());
    }
}
