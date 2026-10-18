package io.github.leoheco.serlog.internal;

import io.github.leoheco.serlog.Colour;
import io.github.leoheco.serlog.Compat;
import io.github.leoheco.serlog.Filter;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.Meta;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.Sink;
import io.github.leoheco.serlog.Slot;
import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.config.Config;
import io.github.leoheco.serlog.process.Processor;
import io.github.leoheco.serlog.process.Redact;
import io.github.leoheco.serlog.sink.ConsoleSink;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.stream.Stream;

/// Start-up: one call, the same keys as a config file, all checked before anything changes.
public final class Start {
    private static final Set<String> KEYS = Set.of(
            "app",
            "env",
            "instance",
            "compat",
            "level",
            "filter",
            "filter_env",
            "levels",
            "process",
            "redact",
            "sinks",
            "alarms",
            "watch");
    private static final Set<String> FORMATS = Set.of("text", "json", "logfmt", "pretty");
    private static final AtomicBoolean STARTED = new AtomicBoolean();

    private Start() {}

    /// Everything checked and built, ready to swap in.
    public record Plan(
            Meta meta,
            Compat compat,
            Filter filter,
            List<Processor> process,
            List<Live> live,
            Alarm alarm,
            Map<String, Alarm> named) {}

    private static Slot slot(Object s) {
        return switch (s) {
            case Slot x -> x;
            case Sink x -> Slot.of(x);
            case Map<?, ?> m -> Config.slot(cast(m));
            default -> throw new SerlogException.Config("a sink is a sink object or a config entry, not " + s);
        };
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> cast(Map<?, ?> m) {
        return (Map<String, Object>) m;
    }

    private static Slot console() {
        var format = System.getenv("SERLOG_FORMAT");
        if (format == null || format.isEmpty()) return Slot.of(new ConsoleSink());
        if (!FORMATS.contains(format))
            throw new SerlogException.Config("SERLOG_FORMAT `" + format + "` is not text, json, logfmt or pretty");
        return Slot.of(new ConsoleSink()).format(format);
    }

    private static List<?> list(Object v, String key) {
        return switch (v) {
            case null -> List.of();
            case List<?> l -> l;
            case Object[] a -> List.of(a);
            default -> throw new SerlogException.Config("`" + key + "` takes a list");
        };
    }

    private static Map<?, ?> map(Object v, String key) {
        return switch (v) {
            case null -> Map.of();
            case Map<?, ?> m -> m;
            default -> throw new SerlogException.Config("`" + key + "` takes a map");
        };
    }

    /// Checks `cfg` and builds its slots, keeping nothing on error.
    public static Plan plan(Map<String, ?> cfg, List<Live> alongside) {
        var bad = cfg.keySet().stream().filter(k -> !KEYS.contains(k)).sorted().findFirst();
        if (bad.isPresent()) throw new SerlogException.Config("unknown key `" + bad.get() + "`");
        try {
            map(cfg.get("levels"), "levels").forEach((name, spec) -> {
                if (spec instanceof Map<?, ?> m)
                    Level.define(
                            String.valueOf(name),
                            ((Number) m.get("value")).intValue(),
                            m.get("colour") == null ? Colour.PLAIN : Colour.parse(String.valueOf(m.get("colour"))));
                else Level.define(String.valueOf(name), ((Number) spec).intValue(), Colour.PLAIN);
            });
            var env = System.getenv("SERLOG_COMPAT");
            var given = cfg.get("compat");
            var profile = env != null && !env.isBlank()
                    ? Compat.parse(env)
                    : given instanceof Compat c
                            ? c
                            : given != null ? Compat.parse(String.valueOf(given)) : Compat.SERLOG;
            var vars = cfg.get("filter_env") != null ? List.of(String.valueOf(cfg.get("filter_env"))) : profile.env();
            var found = vars.stream()
                    .map(System::getenv)
                    .filter(v -> v != null && !v.isBlank())
                    .findFirst();
            var spec = found.<Object>map(String::strip).orElse(cfg.get("filter"));
            var level = cfg.get("level");
            var filter = withDebug(
                    spec != null ? Filter.from(spec) : new Filter(level != null ? Level.from(level) : profile.level()));
            var steps = new ArrayList<Processor>();
            for (var s : list(cfg.get("process"), "process")) steps.add(s instanceof Processor p ? p : Config.step(s));
            var redact = list(cfg.get("redact"), "redact");
            if (!redact.isEmpty())
                steps.add(Redact.keys(redact.stream().map(String::valueOf).toArray(String[]::new)));
            var named = new LinkedHashMap<String, Alarm>();
            map(cfg.get("alarms"), "alarms")
                    .forEach((n, a) -> named.put(
                            String.valueOf(n),
                            a instanceof Alarm x ? x : Config.alarm(String.valueOf(n), cast(map(a, "alarms." + n)))));
            var fallback = named.remove("default");
            var slots =
                    list(cfg.get("sinks"), "sinks").stream().map(Start::slot).toList();
            var live = Hub.prepare(slots.isEmpty() ? List.of(console()) : slots, alongside, named);
            var known = Hub.known(live);
            var routes = new ArrayList<Map.Entry<String, Object>>();
            if (fallback != null) routes.add(Map.entry("the default alarm", fallback));
            for (var l : live)
                routes.add(Map.entry("the alarm of `" + l.name() + "`", l.core.alarm == null ? "" : l.core.alarm));
            for (var r : routes)
                if (r.getValue() instanceof Alarm a && a != Hub.DEFAULT) {
                    var miss =
                            a.names().stream().filter(n -> !known.contains(n)).findFirst();
                    if (miss.isPresent())
                        throw new SerlogException.Config(
                                r.getKey() + " routes to `" + miss.get() + "`, which is not a sink");
                }
            var m = Meta.detect();
            var meta = new Meta(
                    str(cfg.get("app"), m.app()), str(cfg.get("env"), m.env()), str(cfg.get("instance"), m.instance()));
            return new Plan(meta, profile, filter, List.copyOf(steps), live, fallback, named);
        } catch (SerlogException e) {
            throw e;
        } catch (RuntimeException e) {
            throw new SerlogException.Config(SerlogException.message(e), e);
        }
    }

    private static String str(Object v, String or) {
        return v == null ? or : String.valueOf(v);
    }

    /// `filter` with the `DEBUG` namespaces added, ahead of any `/regex` part.
    private static Filter withDebug(Filter filter) {
        var dbg = System.getenv("DEBUG");
        if (dbg == null || dbg.isBlank()) return filter;
        var extra = String.join(
                ",",
                Stream.of(dbg.split("[\\s,]+"))
                        .filter(s -> !s.isEmpty())
                        .map(ns -> ns.startsWith("-") ? ns.substring(1) + "=off" : ns + "=debug")
                        .toList());
        var text = filter.toString();
        var cut = text.indexOf('/');
        var head = cut < 0 ? text : text.substring(0, cut);
        var tail = cut < 0 ? "" : text.substring(cut);
        return new Filter((head.isEmpty() ? extra : head + "," + extra) + tail);
    }

    /// Puts a plan in force, adding to the slots or replacing them.
    public static void apply(Plan p, boolean replace) {
        Hub.setMeta(p.meta());
        Hub.setCompat(p.compat());
        Hub.setFilter(p.filter());
        Hub.setSteps(p.process());
        if (p.alarm() != null) Hub.setAlarm(p.alarm());
        Export.name(p.named());
        if (replace) Hub.replace(p.live());
        else Hub.install(p.live());
    }

    /// Whether serlog was started.
    public static boolean started() {
        return STARTED.get();
    }

    /// Starts from settings already in hand, as `load` does.
    public static void start(Map<String, ?> cfg) {
        if (!STARTED.compareAndSet(false, true)) throw new SerlogException.Twice();
        try {
            apply(plan(cfg, Hub.table()), false);
        } catch (RuntimeException e) {
            STARTED.set(false);
            throw e;
        }
    }

    /// Starts from the file `SERLOG_CONFIG` names or the first `serlog.*` in the working directory, else a console.
    public static void init() {
        if (STARTED.get()) throw new SerlogException.Twice();
        var env = System.getenv("SERLOG_CONFIG");
        var path = env != null && !env.isEmpty()
                ? Path.of(env)
                : Stream.of("serlog.yaml", "serlog.yml", "serlog.json", "serlog.toml")
                        .map(Path::of)
                        .filter(Files::exists)
                        .findFirst()
                        .orElse(null);
        if (path != null) Config.load(path);
        else start(Map.of());
    }
}
