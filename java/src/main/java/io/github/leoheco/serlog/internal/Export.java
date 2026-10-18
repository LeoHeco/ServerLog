package io.github.leoheco.serlog.internal;

import io.github.leoheco.serlog.Colour;
import io.github.leoheco.serlog.Compat;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.alarm.Tiers;
import io.github.leoheco.serlog.layout.JsonLayout;
import io.github.leoheco.serlog.layout.Layouts;
import io.github.leoheco.serlog.layout.PatternLayout;
import io.github.leoheco.serlog.process.Processor;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/// The settings in force as a config tree.
public final class Export {
    private static volatile Map<String, Alarm> named = Map.of();

    private Export() {}

    /// Remembers the names alarms were given in config.
    public static void name(Map<String, Alarm> alarms) {
        named = new LinkedHashMap<>(alarms);
    }

    private static List<Object> steps(List<Processor> list, String whose) {
        return list.stream()
                .<Object>map(s -> s.options()
                        .orElseThrow(
                                () -> new SerlogException.Config("a processor in " + whose + " cannot be exported")))
                .toList();
    }

    private static void layout(Live l, Map<String, Object> out) {
        var lay = l.layout;
        if (lay == null) return;
        switch (lay) {
            case PatternLayout p -> out.put("pattern", p.source());
            case JsonLayout j -> {
                out.put("format", "json");
                if (j.schema() != JsonLayout.Schema.SERLOG)
                    out.put("schema", j.schema().id());
                if (!j.keys().isEmpty()) out.put("keys", new LinkedHashMap<>(j.keys()));
                if (j.levels() != null) out.put("levels", j.levels().id());
                if (j.nest() != null) out.put("nest", j.nest());
            }
            default -> {
                if (lay == Layouts.LOGFMT) out.put("format", "logfmt");
                else if (lay == Layouts.PRETTY) out.put("format", "pretty");
                else throw new SerlogException.Config("the layout of sink `" + l.name() + "` cannot be exported");
            }
        }
    }

    private static String onFail(Core c, Map<String, Alarm> extra) {
        if (c.panic) return "panic";
        if (c.alarm == null) return "ignore";
        if (c.alarm == Hub.DEFAULT) return null;
        for (var e : named.entrySet()) if (e.getValue() == c.alarm) return e.getKey();
        for (var e : extra.entrySet()) if (e.getValue() == c.alarm) return e.getKey();
        var n = "alarm-" + (extra.size() + 1);
        extra.put(n, c.alarm);
        return n;
    }

    private static Map<String, Object> sink(Live l, Map<String, Alarm> extra) {
        var c = l.core;
        var own = c.sink.options()
                .orElseThrow(() -> new SerlogException.Config("sink `" + l.name() + "` cannot be exported"));
        var out = new LinkedHashMap<String, Object>();
        out.put("kind", c.kind);
        out.put("name", l.name());
        out.putAll(own);
        if (l.filter != null) out.put("filter", l.filter.toString());
        if (!l.process.isEmpty()) out.put("process", steps(l.process, "sink `" + l.name() + "`"));
        layout(l, out);
        if (l.colour != Colour.When.AUTO) out.put("colour", l.colour.id());
        if (!l.colours.overrides().isEmpty()) {
            var theme = new LinkedHashMap<String, Object>();
            l.colours.overrides().forEach((lv, col) -> theme.put(lv.id(), col.id()));
            out.put("colours", theme);
        }
        if (l.alarmOnly) out.put("alarm_only", true);
        var policy = onFail(c, extra);
        if (policy != null) out.put("on_fail", policy);
        var k = l.keys;
        if (k.containsKey("queue") || k.containsKey("batch") || k.containsKey("wait") || k.containsKey("full")) {
            var q = l.queue;
            if (q == null) out.put("queue", 0);
            else {
                out.put("queue", q.size());
                out.put("batch", q.batch());
                out.put("wait", Units.showTime(q.delay()));
                if (q.full() != io.github.leoheco.serlog.Queued.Full.DROP)
                    out.put("full", q.full().id());
            }
        }
        if (l.retries != null) out.put("retries", l.retries);
        if (l.probe != 5) out.put("probe", Units.showTime(l.probe));
        return out;
    }

    /// The settings in force.
    public static Map<String, Object> tree() {
        var m = Hub.meta();
        var out = new LinkedHashMap<String, Object>();
        if (!m.app().isEmpty()) out.put("app", m.app());
        if (!m.env().isEmpty()) out.put("env", m.env());
        if (!m.instance().isEmpty()) out.put("instance", m.instance());
        if (Hub.compat() != Compat.SERLOG) out.put("compat", Hub.compat().id());
        var levels = new LinkedHashMap<String, Object>();
        io.github.leoheco.serlog.Level.defined()
                .forEach((lv, e) -> levels.put(
                        e.getKey(),
                        e.getValue() == Colour.PLAIN
                                ? (Object) lv.value()
                                : Map.of(
                                        "value",
                                        lv.value(),
                                        "colour",
                                        e.getValue().id())));
        if (!levels.isEmpty()) out.put("levels", levels);
        out.put("filter", Hub.filter().toString());
        var process = steps(Hub.steps(), "the global chain");
        if (!process.isEmpty()) out.put("process", process);
        var extra = new LinkedHashMap<String, Alarm>();
        out.put("sinks", Hub.table().stream().<Object>map(l -> sink(l, extra)).toList());
        var alarms = new LinkedHashMap<String, Object>();
        var def = Hub.current()
                .options()
                .orElseThrow(() -> new SerlogException.Config("the default alarm cannot be exported"));
        if (!def.equals(new Tiers().options().orElseThrow())) alarms.put("default", def);
        for (var source : List.of(named, extra))
            source.forEach((n, a) -> alarms.put(
                    n,
                    a.options().orElseThrow(() -> new SerlogException.Config("alarm `" + n + "` cannot be exported"))));
        if (!alarms.isEmpty()) out.put("alarms", alarms);
        return out;
    }
}
