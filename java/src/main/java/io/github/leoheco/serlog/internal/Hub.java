package io.github.leoheco.serlog.internal;

import io.github.leoheco.serlog.Compat;
import io.github.leoheco.serlog.Field;
import io.github.leoheco.serlog.Filter;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.Meta;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.Slot;
import io.github.leoheco.serlog.Status;
import io.github.leoheco.serlog.Theme;
import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.alarm.Incident;
import io.github.leoheco.serlog.alarm.Tiers;
import io.github.leoheco.serlog.layout.Layout;
import io.github.leoheco.serlog.process.Processor;
import io.github.leoheco.serlog.sink.ConsoleSink;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

/// The hub: global filter, scope context, global processors, the slot table and dispatch. Like most loggers it
/// starts at `info` and prints to standard output until it is configured.
public final class Hub {
    private static volatile Filter filter = new Filter(Level.INFO);
    private static volatile Level floor = Level.INFO;
    private static volatile List<Processor> steps = List.of();
    private static volatile List<Live> table = List.of();
    private static volatile boolean configured;
    private static volatile boolean caller;
    private static volatile Compat compat = Compat.SERLOG;
    private static volatile Meta meta = Meta.detect();
    private static volatile Alarm alarm;
    private static Live fallback;
    private static Live reserved;
    private static boolean hooked;
    private static final Object EDIT = new Object();
    private static final ScopedValue<List<Field>> SCOPE = ScopedValue.newInstance();

    private Hub() {}

    /// What every slot without an `on_fail` reports to: the default alarm in force at the time.
    public static final Alarm DEFAULT = new Alarm() {
        @Override
        public Fallback fallback() {
            return current().fallback();
        }

        @Override
        public Duration remind() {
            return current().remind();
        }

        @Override
        public List<List<String>> route(Incident incident) {
            return current().route(incident);
        }

        @Override
        public LogRecord describe(Incident i) {
            return current().describe(i);
        }

        @Override
        public List<String> names() {
            return current().names();
        }

        @Override
        public Optional<Map<String, Object>> options() {
            return current().options();
        }
    };

    /// The default alarm, made on first use.
    public static Alarm current() {
        var a = alarm;
        if (a == null)
            synchronized (EDIT) {
                if (alarm == null) alarm = new Tiers();
                a = alarm;
            }
        return a;
    }

    /// Replaces the default alarm.
    public static void setAlarm(Alarm a) {
        alarm = a;
    }

    /// The alarm in force behind `a`, which differs only for [#DEFAULT].
    public static Alarm resolve(Alarm a) {
        return a == DEFAULT ? current() : a;
    }

    public static Compat compat() {
        return compat;
    }

    public static void setCompat(Compat c) {
        compat = c;
    }

    public static Meta meta() {
        return meta;
    }

    public static void setMeta(Meta m) {
        meta = m;
    }

    /// Whether a record at `level` could pass the filter for some target; the calls check this first.
    public static boolean enabled(Level level) {
        var f = floor;
        return f != null && level.atLeast(f);
    }

    /// Whether some slot may show the caller, so records need one.
    public static boolean caller() {
        return caller;
    }

    public static Filter filter() {
        return filter;
    }

    public static void setFilter(Filter f) {
        filter = f;
        floor = f.floor();
    }

    public static List<Processor> steps() {
        return steps;
    }

    public static void setSteps(List<Processor> s) {
        steps = List.copyOf(s);
    }

    /// Adds a step to the end of the global chain.
    public static void process(Processor p) {
        synchronized (EDIT) {
            var s = new ArrayList<>(steps);
            s.add(p);
            steps = List.copyOf(s);
        }
    }

    /// The fields bound to this scope.
    public static List<Field> bound() {
        return SCOPE.isBound() ? SCOPE.get() : List.of();
    }

    /// Runs `fn` with `fields` added to every record made inside it.
    public static <T, X extends Throwable> T bind(Map<String, ?> fields, ScopedValue.CallableOp<T, X> fn) throws X {
        var all = new ArrayList<>(bound());
        fields.forEach((k, v) -> all.add(new Field(k, v)));
        return ScopedValue.where(SCOPE, List.copyOf(all)).call(fn);
    }

    public static List<Live> table() {
        return table;
    }

    /// Sends `rec` through the filter, the scope and the global processors, then to every slot on the log route.
    public static void emit(LogRecord rec) {
        if (Guard.suppress()) return;
        var f = filter;
        if (!f.allows(rec.level(), rec.target()) || f.hasPattern() && !f.admits(rec.msg())) return;
        var r = rec;
        var extra = bound();
        if (!extra.isEmpty()) {
            var all = new ArrayList<Field>(extra.size() + rec.fields().size());
            all.addAll(extra);
            all.addAll(rec.fields());
            r = rec.withFields(all);
        }
        var chain = steps;
        if (!chain.isEmpty()) {
            var start = r;
            try {
                r = Guard.call(null, () -> Processor.run(chain, start));
            } catch (RuntimeException e) {
                throw e;
            } catch (Exception e) {
                throw new IllegalStateException(e);
            }
        }
        if (r != null) dispatch(r);
    }

    /// Hands `rec` to every slot on the log route.
    public static void dispatch(LogRecord rec) {
        if (!configured) {
            defaultConsole().emit(rec, null);
            return;
        }
        var t = table;
        Map<Layout, Map<Theme, String>> cache = t.size() > 1 ? new HashMap<>() : null;
        for (var l : t) if (!l.alarmOnly) l.emit(rec, cache);
    }

    private static Live defaultConsole() {
        synchronized (EDIT) {
            if (fallback == null) fallback = new Live(Slot.of(new ConsoleSink()), "console", Map.of());
            return fallback;
        }
    }

    /// The reserved console on standard error, where the default alarm goes.
    static Live stderr() {
        synchronized (EDIT) {
            if (reserved == null)
                reserved = new Live(
                        Slot.of(new ConsoleSink().stream(Compat.Output.STDERR)).onFail("ignore"), "stderr", Map.of());
            return reserved;
        }
    }

    /// Names, checks and wraps new slots against those already registered; nothing is kept on error.
    public static List<Live> prepare(List<Slot> slots, List<Live> existing, Map<String, Alarm> named) {
        var names = new LinkedHashSet<String>();
        existing.forEach(l -> names.add(l.name()));
        names.add("*");
        var claims = new LinkedHashSet<String>();
        for (var l : existing) if (l.claim != null) claims.add(l.claim);
        var counts = new HashMap<String, Integer>();
        for (var l : existing) counts.merge(l.core.kind, 1, Integer::sum);
        var out = new ArrayList<Live>();
        for (var slot : slots) {
            var kind = slot.sink().kind();
            var name = slot.keys().get("name") instanceof String s ? s : null;
            while (name == null) {
                var n = counts.merge(kind, 1, Integer::sum);
                if (!names.contains(kind + "-" + n)) name = kind + "-" + n;
            }
            if (!names.add(name)) throw new SerlogException.NameTaken("sink name `" + name + "` is taken or reserved");
            var live = new Live(slot, name, named);
            if (live.claim != null && !claims.add(live.claim))
                throw new SerlogException.Claimed(live.claim + " is already used by another sink");
            try {
                slot.sink().ready();
            } catch (Exception e) {
                throw new SerlogException.NotReady(name, e);
            }
            out.add(live);
        }
        return out;
    }

    /// Every name an alarm may route to: registered slots, `stderr` and `*`.
    public static Set<String> known(List<Live> extra) {
        var out = new LinkedHashSet<String>();
        table.forEach(l -> out.add(l.name()));
        extra.forEach(l -> out.add(l.name()));
        out.add("stderr");
        out.add("*");
        return out;
    }

    /// Puts prepared slots in the table, after those already there.
    public static void install(List<Live> live) {
        synchronized (EDIT) {
            var t = new ArrayList<>(table);
            t.addAll(live);
            swap(t);
        }
    }

    /// Swaps the whole table, for reloading a config.
    public static void replace(List<Live> live) {
        List<Live> old;
        synchronized (EDIT) {
            old = table;
            swap(live);
        }
        old.forEach(Live::retire);
    }

    /// Registers one slot at run time; returns the name it got.
    public static String add(Slot slot) {
        synchronized (EDIT) {
            var live = prepare(List.of(slot), table, Map.of()).getFirst();
            var t = new ArrayList<>(table);
            t.add(live);
            swap(t);
            return live.name();
        }
    }

    /// Takes a slot out; what its queue holds is still written. `false` when no slot has that name.
    public static boolean remove(String name) {
        List<Live> gone;
        synchronized (EDIT) {
            gone = table.stream().filter(l -> l.name().equals(name)).toList();
            if (gone.isEmpty()) return false;
            swap(table.stream().filter(l -> !l.name().equals(name)).toList());
        }
        gone.forEach(Live::retire);
        return true;
    }

    private static void swap(List<Live> t) {
        table = List.copyOf(t);
        caller = t.stream().anyMatch(Live::wantsCaller);
        configured = true;
        if (!hooked) {
            hooked = true;
            Runtime.getRuntime()
                    .addShutdownHook(Thread.ofPlatform().name("serlog-exit").unstarted(() -> {
                        if (!AlarmTask.crashing()) flush(Duration.ofSeconds(5));
                    }));
        }
    }

    /// The slot called `name`, the reserved `stderr` included.
    static Live find(String name) {
        for (var l : table) if (l.name().equals(name)) return l;
        return name.equals("stderr") ? stderr() : null;
    }

    /// Every slot's state.
    public static List<Status> status() {
        return table.stream().map(Live::status).toList();
    }

    /// Waits up to `within` for every queue and then the alarm route to drain.
    public static void flush(Duration within) {
        var until = System.nanoTime() + within.toNanos();
        flushSlots(within);
        AlarmTask.settle(Duration.ofNanos(Math.max(until - System.nanoTime(), 0)));
    }

    /// Waits up to `within` for every slot.
    public static void flushSlots(Duration within) {
        var until = System.nanoTime() + within.toNanos();
        for (var l : table) l.flush(Duration.ofNanos(Math.max(until - System.nanoTime(), 0)));
    }

    /// The registered slots, as the alarm walk sees them.
    public static final Walk.Desk BOARD = new Walk.Desk() {
        @Override
        public List<String> logs() {
            return table.stream().filter(l -> !l.alarmOnly).map(Live::name).toList();
        }

        @Override
        public boolean resting(String name) {
            var l = find(name);
            return l != null && l.core.resting();
        }

        @Override
        public void deliver(String name, LogRecord rec) throws Exception {
            var l = find(name);
            if (l == null) throw new SerlogException("no sink called `" + name + "`");
            l.core.deliver(rec);
        }
    };
}
