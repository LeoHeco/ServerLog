package serlog

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"time"

	"github.com/LeoHeco/ServerLog/go/internal/tree"
	"github.com/pelletier/go-toml/v2"
	"go.yaml.in/yaml/v3"
)

// Format is config file syntax: "yaml", "json" or "toml".
type Format string

// The formats.
const (
	FormatYAML Format = "yaml"
	FormatJSON Format = "json"
	FormatTOML Format = "toml"
)

// FormatOf is the format a file's extension names; YAML for anything but .json and .toml.
func FormatOf(path string) Format {
	switch strings.ToLower(filepath.Ext(path)) {
	case ".json":
		return FormatJSON
	case ".toml":
		return FormatTOML
	}
	return FormatYAML
}

var topKeys = []string{"app", "env", "instance", "compat", "level", "filter", "filter_env", "levels", "process",
	"redact", "sinks", "alarms", "watch"}

var slotKeys = []string{"name", "level", "filter", "process", "layout", "format", "pattern", "schema", "keys",
	"levels", "nest", "colour", "colours", "alarm_only", "on_fail", "queue", "batch", "wait", "full", "retries", "probe"}

// normalise gives every format the same tree: integers as int64, maps keyed by strings.
func normalise(v any) any {
	switch x := v.(type) {
	case json.Number:
		if n, err := x.Int64(); err == nil {
			return n
		}
		f, _ := x.Float64()
		return f
	case int:
		return int64(x)
	case uint64:
		return int64(x)
	case map[string]any:
		for k, e := range x {
			x[k] = normalise(e)
		}
		return x
	case map[any]any:
		out := make(map[string]any, len(x))
		for k, e := range x {
			out[fmt.Sprint(k)] = normalise(e)
		}
		return out
	case []any:
		for i, e := range x {
			x[i] = normalise(e)
		}
		return x
	}
	return v
}

// Read is config text as a tree, without expanding variables.
func Read(text string, f Format) (map[string]any, error) {
	var out map[string]any
	var err error
	switch f {
	case FormatJSON:
		d := json.NewDecoder(strings.NewReader(text))
		d.UseNumber()
		if strings.TrimSpace(text) != "" {
			err = d.Decode(&out)
		}
	case FormatTOML:
		err = toml.Unmarshal([]byte(text), &out)
	default:
		err = yaml.Unmarshal([]byte(text), &out)
	}
	if err != nil {
		return nil, &ConfigError{Msg: fmt.Sprintf("bad %s: %v", f, err), Err: err}
	}
	if out == nil {
		out = map[string]any{}
	}
	return normalise(out).(map[string]any), nil
}

var variable = regexp.MustCompile(`\$\{([^}]*)\}`)

// Expand replaces `${NAME}` in every string with that environment variable; an unset one is an error.
func Expand(v any) (any, error) {
	switch x := v.(type) {
	case string:
		var err error
		out := variable.ReplaceAllStringFunc(x, func(m string) string {
			name := m[2 : len(m)-1]
			val, ok := os.LookupEnv(name)
			if !ok && err == nil {
				err = configErr("environment variable %s is not set", name)
			}
			return val
		})
		return out, err
	case map[string]any:
		out := make(map[string]any, len(x))
		for k, e := range x {
			n, err := Expand(e)
			if err != nil {
				return nil, err
			}
			out[k] = n
		}
		return out, nil
	case []any:
		out := make([]any, len(x))
		for i, e := range x {
			n, err := Expand(e)
			if err != nil {
				return nil, err
			}
			out[i] = n
		}
		return out, nil
	}
	return v, nil
}

func list(v any, key string) ([]any, error) {
	switch x := v.(type) {
	case nil:
		return nil, nil
	case []any:
		return x, nil
	}
	return nil, configErr("`%s` takes a list", key)
}

func table(v any, key string) (map[string]any, error) {
	switch x := v.(type) {
	case nil:
		return nil, nil
	case map[string]any:
		return x, nil
	}
	return nil, configErr("`%s` takes a map", key)
}

func levelOf(v any) (Level, error) {
	if s, ok := v.(string); ok {
		l, err := ParseLevel(s)
		if err != nil {
			return 0, &ConfigError{Msg: err.Error(), Err: err}
		}
		return l, nil
	}
	if f, ok := toFloat(v); ok && f >= 0 && f <= 255 {
		return Level(f), nil
	}
	return 0, configErr("not a level: %v", v)
}

// Step is the processor a `process` entry describes: a name, or a map from a name to its options.
func Step(spec any) (Processor, error) {
	var name string
	var opts map[string]any
	switch x := spec.(type) {
	case string:
		name = x
	case map[string]any:
		if len(x) != 1 {
			return nil, configErr("a processor is a name or a one-key map")
		}
		for k, v := range x {
			name = k
			switch o := v.(type) {
			case nil, bool:
			case map[string]any:
				opts = o
			default:
				return nil, configErr("processor `%s` takes a map", k)
			}
		}
	default:
		return nil, configErr("a processor is a name or a one-key map, not %v", spec)
	}
	fn, err := lookupKind(stepKinds, "processor", name)
	if err != nil {
		return nil, err
	}
	return build(fmt.Sprintf("processor `%s`", name), fn, opts)
}

func slotFrom(m map[string]any) (Slot, error) {
	o := newOptions("sink", m)
	s := Slot{
		Name: o.String("name"), Filter: o.String("filter"), Format: o.String("format"), Pattern: o.String("pattern"),
		Schema: o.String("schema"), Keys: o.Table("keys"), Levels: o.String("levels"), Nest: o.String("nest"),
		Colour: o.String("colour"), AlarmOnly: o.Bool("alarm_only"), OnFail: o.String("on_fail"),
		Batch: o.Int("batch"), Wait: o.Duration("wait"), Full: o.String("full"), Probe: o.Duration("probe"),
	}
	if l := o.String("layout"); strings.Contains(l, "{") {
		s.Pattern = l
	} else {
		s.LayoutName = l
	}
	if v := o.Raw("level"); v != nil {
		l, err := levelOf(v)
		if err != nil {
			return s, err
		}
		s.Level = l
	}
	if o.Has("queue") {
		s.Queue = cmp0(o.Int("queue"), Direct)
	}
	if o.Has("retries") {
		s.Retries = cmp0(o.Int("retries"), NoRetries)
	}
	if v := o.Raw("colours"); v != nil {
		c, ok := v.(map[string]any)
		if !ok {
			return s, configErr("`colours` takes a map")
		}
		s.Colours = Theme{}
		for k, name := range c {
			l, err := levelOf(k)
			if err != nil {
				return s, err
			}
			col, err := ParseColour(fmt.Sprint(name))
			if err != nil {
				return s, &ConfigError{Msg: err.Error(), Err: err}
			}
			s.Colours[l] = col
		}
	}
	steps, err := list(o.Raw("process"), "process")
	if err != nil {
		return s, err
	}
	for _, spec := range steps {
		p, err := Step(spec)
		if err != nil {
			return s, err
		}
		s.Process = append(s.Process, p)
	}
	return s, o.done()
}

// cmp0 is fallback when v is zero, so a config's explicit 0 means none.
func cmp0(v, fallback int) int {
	if v == 0 {
		return fallback
	}
	return v
}

// SinkFrom is the sink a config entry describes, with its slot settings.
func SinkFrom(entry map[string]any) (Sink, error) {
	kind, ok := entry["kind"].(string)
	if !ok {
		return nil, configErr("each sink needs a `kind`")
	}
	own, slot := map[string]any{}, map[string]any{}
	for k, v := range entry {
		switch {
		case k == "kind":
		case slices.Contains(slotKeys, k):
			slot[k] = v
		default:
			own[k] = v
		}
	}
	fn, err := lookupKind(sinkKinds, "sink", kind)
	if err != nil {
		return nil, err
	}
	sink, err := build(kind+" sink", fn, own)
	if err != nil {
		return nil, err
	}
	s, err := slotFrom(slot)
	if err != nil {
		return nil, err
	}
	return With(sink, s), nil
}

// AlarmFrom is the alarm an `alarms` entry describes.
func AlarmFrom(name string, spec map[string]any) (Alarm, error) {
	if name == "panic" || name == "ignore" {
		return nil, configErr("`%s` cannot name an alarm", name)
	}
	keys := map[string]any{}
	kind := "tiers"
	for k, v := range spec {
		if k == "kind" {
			kind = fmt.Sprint(v)
		} else {
			keys[k] = v
		}
	}
	fn, err := lookupKind(alarmKinds, "alarm", kind)
	if err != nil {
		return nil, err
	}
	return build(fmt.Sprintf("alarm `%s`", name), fn, keys)
}

// Parse turns a config tree into settings for Init.
func Parse(t map[string]any) (Config, error) {
	var c Config
	for k := range t {
		if !slices.Contains(topKeys, k) {
			return c, configErr("unknown key `%s`", k)
		}
	}
	str := func(k string) string {
		if v, ok := t[k]; ok && v != nil {
			return fmt.Sprint(v)
		}
		return ""
	}
	c.App, c.Env, c.Instance, c.Filter, c.FilterEnv = str("app"), str("env"), str("instance"), str("filter"), str("filter_env")
	c.Compat = Compat(str("compat"))
	if v, ok := t["level"]; ok {
		l, err := levelOf(v)
		if err != nil {
			return c, err
		}
		c.Level = l
	}
	if v, ok := t["watch"]; ok {
		d, err := duration(v)
		if err != nil {
			return c, &ConfigError{Msg: err.Error(), Err: err}
		}
		c.Watch = d
	}
	levels, err := table(t["levels"], "levels")
	if err != nil {
		return c, err
	}
	for name, spec := range levels {
		d := LevelDef{}
		switch x := spec.(type) {
		case map[string]any:
			l, err := levelOf(x["value"])
			if err != nil {
				return c, err
			}
			d.Value = l
			if col, ok := x["colour"]; ok {
				if d.Colour, err = ParseColour(fmt.Sprint(col)); err != nil {
					return c, &ConfigError{Msg: err.Error(), Err: err}
				}
			}
		default:
			l, err := levelOf(x)
			if err != nil {
				return c, err
			}
			d.Value = l
		}
		if c.Levels == nil {
			c.Levels = map[string]LevelDef{}
		}
		c.Levels[name] = d
	}
	for name, d := range c.Levels {
		if err := DefineLevel(name, d.Value, d.Colour); err != nil {
			return c, &ConfigError{Msg: err.Error(), Err: err}
		}
	}
	steps, err := list(t["process"], "process")
	if err != nil {
		return c, err
	}
	for _, s := range steps {
		p, err := Step(s)
		if err != nil {
			return c, err
		}
		c.Process = append(c.Process, p)
	}
	redact, err := list(t["redact"], "redact")
	if err != nil {
		return c, err
	}
	for _, k := range redact {
		c.Redact = append(c.Redact, fmt.Sprint(k))
	}
	alarms, err := table(t["alarms"], "alarms")
	if err != nil {
		return c, err
	}
	for name, spec := range alarms {
		m, err := table(spec, "alarms."+name)
		if err != nil {
			return c, err
		}
		a, err := AlarmFrom(name, m)
		if err != nil {
			return c, err
		}
		if c.Alarms == nil {
			c.Alarms = map[string]Alarm{}
		}
		c.Alarms[name] = a
	}
	sinks, err := list(t["sinks"], "sinks")
	if err != nil {
		return c, err
	}
	for _, e := range sinks {
		m, err := table(e, "sinks")
		if err != nil {
			return c, err
		}
		s, err := SinkFrom(m)
		if err != nil {
			return c, err
		}
		c.Sinks = append(c.Sinks, s)
	}
	return c, nil
}

func parseText(text string, f Format) (Config, error) {
	t, err := Read(text, f)
	if err != nil {
		return Config{}, err
	}
	e, err := Expand(t)
	if err != nil {
		return Config{}, err
	}
	return Parse(e.(map[string]any))
}

// Loads starts serlog from config text.
func Loads(text string, f Format) error {
	c, err := parseText(text, f)
	if err != nil {
		return err
	}
	return start(c)
}

// Load starts serlog from a file, the format from its extension, and reloads it if it sets `watch`.
func Load(path string) error {
	text, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	c, err := parseText(string(text), FormatOf(path))
	if err != nil {
		return err
	}
	if err := start(c); err != nil {
		return err
	}
	if c.Watch > 0 {
		go watch(path, c.Watch)
	}
	return nil
}

func stamp(path string) time.Time {
	st, err := os.Stat(path)
	if err != nil {
		return time.Time{}
	}
	return st.ModTime()
}

// watch reloads path whenever its modification time changes, if all of it passes; a failure goes to stderr.
func watch(path string, every time.Duration) {
	seen := stamp(path)
	for range time.Tick(every) {
		if now := stamp(path); !now.Equal(seen) {
			seen = now
			if err := reload(path); err != nil {
				r := NewRecord(LevelError, "serlog", fmt.Sprintf("reloading %s failed: %v", path, err), Field{"error", err})
				if board.deliver(board{}, "stderr", r) != nil {
					fmt.Fprintln(os.Stderr, r.Msg)
				}
			}
		}
	}
}

func reload(path string) error {
	text, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	c, err := parseText(string(text), FormatOf(path))
	if err != nil {
		return err
	}
	p, err := makePlan(c, nil)
	if err != nil {
		return err
	}
	apply(p, true)
	return nil
}

// Dump is the settings in force as config text that loads back to the same behaviour; passwords come out as
// `***`. A custom sink, processor, layout or alarm without Options cannot be dumped, and the error names it.
func Dump(f Format) (string, error) {
	t, err := export()
	if err != nil {
		return "", err
	}
	switch f {
	case FormatJSON:
		return tree.JSON(t), nil
	case FormatTOML:
		return tree.TOML(t), nil
	}
	return tree.YAML(t)
}
