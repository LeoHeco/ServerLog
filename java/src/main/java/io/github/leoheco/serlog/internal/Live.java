package io.github.leoheco.serlog.internal;

import io.github.leoheco.serlog.Colour;
import io.github.leoheco.serlog.Filter;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.Queued;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.Sink;
import io.github.leoheco.serlog.Slot;
import io.github.leoheco.serlog.Status;
import io.github.leoheco.serlog.Theme;
import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.config.Config;
import io.github.leoheco.serlog.layout.Layout;
import io.github.leoheco.serlog.layout.Layouts;
import io.github.leoheco.serlog.process.Processor;
import java.time.Duration;
import java.util.List;
import java.util.Map;

/// A registered slot: its settings, the core both the callers and the queue use, and the queue in front of it.
public final class Live {
    final Core core;
    final Map<String, Object> keys;
    final Filter filter;
    final List<Processor> process;
    final Layout layout;
    final Colour.When colour;
    final Theme colours;
    final boolean alarmOnly;
    final Object onFail;
    final Queued queue;
    final Integer retries;
    final double probe;
    final String claim;
    private final Lane lane;

    Live(Slot slot, String name, Map<String, Alarm> named) {
        var sink = slot.sink();
        var k = slot.keys();
        this.keys = k;
        try {
            if (k.containsKey("level") && k.containsKey("filter"))
                throw new SerlogException.Config("give `level` or `filter`, not both");
            this.filter = k.containsKey("level")
                    ? new Filter(Level.from(k.get("level")))
                    : k.containsKey("filter") ? Filter.from(k.get("filter")) : null;
            this.process = steps(k.get("process"));
            this.layout = Layouts.build(k);
            this.colour = k.get("colour") instanceof Colour.When w
                    ? w
                    : k.containsKey("colour") ? Colour.When.parse(String.valueOf(k.get("colour"))) : Colour.When.AUTO;
            this.colours = k.get("colours") instanceof Theme t
                    ? t
                    : k.get("colours") instanceof Map<?, ?> m ? Theme.parse(m) : Theme.PLAIN;
            this.alarmOnly = Boolean.TRUE.equals(k.get("alarm_only"));
            this.onFail = k.get("on_fail");
            this.queue = mode(sink, k);
            this.retries = k.get("retries") instanceof Number n ? n.intValue() : null;
            this.probe = k.containsKey("probe") ? Units.seconds(k.get("probe")) : 5;
        } catch (SerlogException e) {
            throw e;
        } catch (RuntimeException e) {
            throw new SerlogException.Config("sink `" + name + "`: " + SerlogException.message(e), e);
        }
        var shape = queue;
        var theme = colour.resolve(sink.tty()) ? colours : null;
        this.core = new Core(
                name,
                sink,
                layout != null ? layout : sink.defaultLayout().orElse(Layouts.INHERIT),
                theme,
                policy(onFail, named),
                retries != null ? retries : shape != null ? 3 : 0,
                probe);
        this.claim = sink.claim().orElse(null);
        this.lane = shape != null ? new Lane(shape, core) : null;
    }

    private static List<Processor> steps(Object given) {
        if (given == null) return List.of();
        if (!(given instanceof List<?> list)) throw new SerlogException.Config("`process` takes a list");
        return list.stream()
                .map(s -> s instanceof Processor p ? p : Config.step(s))
                .toList();
    }

    private static Queued mode(Sink sink, Map<String, Object> k) {
        var base = sink.queued().orElse(null);
        if (!k.containsKey("queue") && !k.containsKey("batch") && !k.containsKey("wait") && !k.containsKey("full"))
            return base;
        var size = k.get("queue") instanceof Number n ? n.intValue() : base != null ? base.size() : 1024;
        if (size == 0) return null;
        var start = base != null ? base : Queued.of(size);
        return new Queued(
                size,
                k.get("batch") instanceof Number n ? n.intValue() : start.batch(),
                k.containsKey("wait") ? Units.duration(k.get("wait")) : start.delay(),
                k.get("full") instanceof Queued.Full f
                        ? f
                        : k.containsKey("full") ? Queued.Full.parse(String.valueOf(k.get("full"))) : start.full());
    }

    /// The failure policy an `on_fail` setting names.
    static Object policy(Object onFail, Map<String, Alarm> named) {
        if (onFail == null || "default".equals(onFail)) return Hub.DEFAULT;
        if ("panic".equals(onFail) || "ignore".equals(onFail) || onFail instanceof Alarm) return onFail;
        var found = onFail instanceof String s ? named.get(s) : null;
        if (found == null)
            throw new SerlogException.Config("`on_fail` names `" + onFail + "`, which is not in `alarms`");
        return found;
    }

    /// Its name.
    public String name() {
        return core.name;
    }

    /// Its sink.
    public Sink sink() {
        return core.sink;
    }

    /// Whether lines may show the caller.
    boolean wantsCaller() {
        return core.layout.caller();
    }

    /// Takes one record from the hub.
    void emit(LogRecord rec, Map<Layout, Map<Theme, String>> cache) {
        if (filter != null && !filter.allows(rec.level(), rec.target())) return;
        if (!core.sink.enabled(rec.level(), rec.target())) return;
        var r = rec;
        if (!process.isEmpty()) {
            var steps = process;
            try {
                r = Guard.call(null, () -> Processor.run(steps, rec));
            } catch (RuntimeException e) {
                throw e;
            } catch (Exception e) {
                throw new IllegalStateException(e);
            }
            if (r == null) return;
        }
        if (lane != null) lane.push(r);
        else core.write(core.entry(r, r == rec ? cache : null));
    }

    /// Waits for the queue, or flushes the sink.
    void flush(Duration within) {
        if (lane != null) lane.flush(within);
        else core.flush();
    }

    /// Writes what the queue holds, stops its worker and closes the sink.
    void retire() {
        flush(Duration.ofSeconds(5));
        if (lane != null) lane.stop();
        core.close();
    }

    /// The slot's state.
    Status status() {
        return core.status();
    }

    /// The core, for the alarm walk.
    Core core() {
        return core;
    }
}
