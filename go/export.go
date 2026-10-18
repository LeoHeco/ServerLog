package serlog

import (
	"maps"
	"reflect"
	"slices"
	"strconv"
	"sync"
	"time"

	"github.com/LeoHeco/ServerLog/go/internal/tree"
)

var (
	namedMu sync.Mutex
	named   map[string]Alarm
)

func nameAlarms(m map[string]Alarm) {
	namedMu.Lock()
	defer namedMu.Unlock()
	named = maps.Clone(m)
}

func same(a, b Alarm) bool {
	ta, tb := reflect.TypeOf(a), reflect.TypeOf(b)
	return ta == tb && ta.Comparable() && a == b
}

func stepOptions(steps []Processor, whose string) ([]any, error) {
	out := make([]any, len(steps))
	for i, s := range steps {
		e, ok := s.(exporter)
		if !ok {
			return nil, configErr("a processor in %s cannot be exported", whose)
		}
		out[i] = e.Options()
	}
	return out, nil
}

func ordered(m map[string]any) tree.Map {
	out := tree.Map{}
	for _, k := range slices.Sorted(maps.Keys(m)) {
		out = append(out, tree.KV{Key: k, Value: m[k]})
	}
	return out
}

func layoutKeys(l *live, out *tree.Map) error {
	switch x := l.layout.(type) {
	case nil:
	case *Pattern:
		out.Set("pattern", x.source)
	case *JSON:
		out.Set("format", "json")
		if x.Schema != "" && x.Schema != "serlog" {
			out.Set("schema", x.Schema)
		}
		if len(x.Keys) > 0 {
			out.Set("keys", x.Keys)
		}
		if x.Levels != "" {
			out.Set("levels", x.Levels)
		}
		if x.Nest != "" {
			out.Set("nest", x.Nest)
		}
	case logfmt:
		out.Set("format", "logfmt")
	case pretty:
		out.Set("format", "pretty")
	default:
		return configErr("the layout of sink `%s` cannot be exported", l.name())
	}
	return nil
}

func policyName(c *core, extra *tree.Map, alarms map[string]Alarm) string {
	switch {
	case c.panics:
		return "panic"
	case c.alarm == nil:
		return "ignore"
	case same(c.alarm, defaultAlarm):
		return ""
	}
	for _, k := range slices.Sorted(maps.Keys(alarms)) {
		if same(alarms[k], c.alarm) {
			return k
		}
	}
	for _, kv := range *extra {
		if same(kv.Value.(Alarm), c.alarm) {
			return kv.Key
		}
	}
	n := "alarm-" + strconv.Itoa(len(*extra)+1)
	extra.Set(n, c.alarm)
	return n
}

func sinkEntry(l *live, extra *tree.Map, alarms map[string]Alarm) (tree.Map, error) {
	c, s := l.core, l.slot
	e, ok := c.sink.(exporter)
	if !ok {
		return nil, configErr("sink `%s` cannot be exported", l.name())
	}
	out := tree.Map{{Key: "kind", Value: c.kind}, {Key: "name", Value: l.name()}}
	out = append(out, ordered(e.Options())...)
	if l.filter != nil {
		out.Set("filter", l.filter.String())
	}
	if len(s.Process) > 0 {
		steps, err := stepOptions(s.Process, "sink `"+l.name()+"`")
		if err != nil {
			return nil, err
		}
		out.Set("process", steps)
	}
	if err := layoutKeys(l, &out); err != nil {
		return nil, err
	}
	if s.Colour != "" && s.Colour != "auto" {
		out.Set("colour", s.Colour)
	}
	if len(s.Colours) > 0 {
		theme := map[string]any{}
		for lv, col := range s.Colours {
			theme[lv.String()] = col.String()
		}
		out.Set("colours", theme)
	}
	if s.AlarmOnly {
		out.Set("alarm_only", true)
	}
	if p := policyName(c, extra, alarms); p != "" {
		out.Set("on_fail", p)
	}
	if s.Queue != 0 || s.Batch != 0 || s.Wait != 0 || s.Full != "" {
		if q := l.queue; q == nil {
			out.Set("queue", 0)
		} else {
			out.Set("queue", q.Size)
			out.Set("batch", q.Batch)
			out.Set("wait", showTime(q.Wait))
			if q.Full == "block" {
				out.Set("full", "block")
			}
		}
	}
	switch {
	case s.Retries == NoRetries:
		out.Set("retries", 0)
	case s.Retries > 0:
		out.Set("retries", s.Retries)
	}
	if s.Probe != 0 && s.Probe != 5*time.Second {
		out.Set("probe", showTime(s.Probe))
	}
	return out, nil
}

func alarmOptions(name string, a Alarm) (any, error) {
	e, ok := a.(exporter)
	if !ok {
		return nil, configErr("alarm `%s` cannot be exported", name)
	}
	return e.Options(), nil
}

// export is the settings in force as an ordered tree.
func export() (tree.Map, error) {
	s := load()
	m := s.meta
	out := tree.Map{}
	for _, kv := range []tree.KV{{Key: "app", Value: m.App}, {Key: "env", Value: m.Env}, {Key: "instance", Value: m.Instance}} {
		if kv.Value != "" {
			out = append(out, kv)
		}
	}
	if s.compat != CompatSerlog {
		out.Set("compat", string(s.compat))
	}
	if defs := definedLevels(); len(defs) > 0 {
		levels := map[string]any{}
		for lv, i := range defs {
			if i.colour == ColourPlain {
				levels[i.name] = int(lv)
			} else {
				levels[i.name] = map[string]any{"value": int(lv), "colour": i.colour.String()}
			}
		}
		out.Set("levels", levels)
	}
	out.Set("filter", s.filter.String())
	if len(s.steps) > 0 {
		steps, err := stepOptions(s.steps, "the global chain")
		if err != nil {
			return nil, err
		}
		out.Set("process", steps)
	}
	namedMu.Lock()
	alarms := maps.Clone(named)
	namedMu.Unlock()
	var extra tree.Map
	sinks := make([]any, len(s.table))
	for i, l := range s.table {
		e, err := sinkEntry(l, &extra, alarms)
		if err != nil {
			return nil, err
		}
		sinks[i] = e
	}
	out.Set("sinks", sinks)
	all := tree.Map{}
	def, err := alarmOptions("default", cur())
	if err != nil {
		return nil, err
	}
	if !reflect.DeepEqual(def, (&Tiers{}).Options()) {
		all.Set("default", def)
	}
	for _, k := range slices.Sorted(maps.Keys(alarms)) {
		o, err := alarmOptions(k, alarms[k])
		if err != nil {
			return nil, err
		}
		all.Set(k, o)
	}
	for _, kv := range extra {
		o, err := alarmOptions(kv.Key, kv.Value.(Alarm))
		if err != nil {
			return nil, err
		}
		all.Set(kv.Key, o)
	}
	if len(all) > 0 {
		out.Set("alarms", all)
	}
	return out, nil
}
