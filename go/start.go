package serlog

import (
	"cmp"
	"fmt"
	"os"
	"slices"
	"strings"
	"sync/atomic"
	"time"
)

// LevelDef defines a custom level: its value and colour.
type LevelDef struct {
	Value  Level
	Colour Colour
}

// Config is what Init takes: the keys of a config file. Zero values mean the default.
type Config struct {
	App       string
	Env       string
	Instance  string
	Compat    Compat
	Level     Level
	Filter    string
	FilterEnv string
	Levels    map[string]LevelDef
	Process   []Processor
	Redact    []string
	Sinks     []Sink
	Alarms    map[string]Alarm
	Watch     time.Duration
}

type plan struct {
	meta   Meta
	compat Compat
	filter *Filter
	steps  []Processor
	live   []*live
	alarm  Alarm
	named  map[string]Alarm
}

var started atomic.Bool

func zeroConfig(c *Config) bool {
	return c.App == "" && c.Env == "" && c.Instance == "" && c.Compat == "" && c.Level == 0 && c.Filter == "" &&
		c.FilterEnv == "" && c.Levels == nil && c.Process == nil && c.Redact == nil && c.Sinks == nil &&
		c.Alarms == nil && c.Watch == 0
}

// Init starts serlog once. A zero Config reads the file SERLOG_CONFIG names, or the first of serlog.yaml,
// serlog.yml, serlog.json and serlog.toml in the working directory; failing that it starts a console sink.
// Everything is checked before anything changes.
func Init(cfg Config) error {
	if started.Load() {
		return ErrTwice
	}
	if zeroConfig(&cfg) {
		path := os.Getenv("SERLOG_CONFIG")
		for _, n := range []string{"serlog.yaml", "serlog.yml", "serlog.json", "serlog.toml"} {
			if _, err := os.Stat(n); path == "" && err == nil {
				path = n
			}
		}
		if path != "" {
			return Load(path)
		}
	}
	return start(cfg)
}

func start(cfg Config) error {
	if !started.CompareAndSwap(false, true) {
		return ErrTwice
	}
	p, err := makePlan(cfg, load().table)
	if err != nil {
		started.Store(false)
		return err
	}
	apply(p, false)
	return nil
}

// Started is whether Init ran.
func Started() bool { return started.Load() }

func withDebug(f *Filter) *Filter {
	var extra []string
	for ns := range strings.FieldsFuncSeq(os.Getenv("DEBUG"), func(r rune) bool { return r == ',' || r == ' ' || r == '\t' }) {
		if n, ok := strings.CutPrefix(ns, "-"); ok {
			extra = append(extra, n+"=off")
		} else {
			extra = append(extra, ns+"=debug")
		}
	}
	if len(extra) == 0 {
		return f
	}
	head, tail, cut := strings.Cut(f.String(), "/")
	spec := strings.Join(slices.DeleteFunc(append([]string{head}, extra...), func(s string) bool { return s == "" }), ",")
	if cut {
		spec += "/" + tail
	}
	return MustFilter(spec)
}

func console() (Sink, error) {
	switch f := os.Getenv("SERLOG_FORMAT"); f {
	case "":
		return &Console{}, nil
	case "text", "json", "logfmt", "pretty":
		return &Console{Slot: Slot{Format: f}}, nil
	default:
		return nil, configErr("SERLOG_FORMAT `%s` is not text, json, logfmt or pretty", f)
	}
}

func makePlan(cfg Config, alongside []*live) (*plan, error) {
	for name, d := range cfg.Levels {
		if err := DefineLevel(name, d.Value, d.Colour); err != nil {
			return nil, &ConfigError{Msg: err.Error(), Err: err}
		}
	}
	profile := cmp.Or(cfg.Compat, CompatSerlog)
	if env := strings.TrimSpace(os.Getenv("SERLOG_COMPAT")); env != "" {
		c, err := ParseCompat(env)
		if err != nil {
			return nil, &ConfigError{Msg: err.Error(), Err: err}
		}
		profile = c
	} else if _, err := ParseCompat(string(profile)); err != nil {
		return nil, &ConfigError{Msg: err.Error(), Err: err}
	}
	vars := profile.Env()
	if cfg.FilterEnv != "" {
		vars = []string{cfg.FilterEnv}
	}
	spec := cfg.Filter
	for _, v := range vars {
		if s := strings.TrimSpace(os.Getenv(v)); s != "" {
			spec = s
			break
		}
	}
	var f *Filter
	switch {
	case spec != "":
		pf, err := ParseFilter(spec)
		if err != nil {
			return nil, &ConfigError{Msg: err.Error(), Err: err}
		}
		f = pf
	case cfg.Level != 0:
		f = LevelFilter(cfg.Level)
	default:
		f = LevelFilter(profile.Level())
	}
	f = withDebug(f)
	steps := slices.Clone(cfg.Process)
	if len(cfg.Redact) > 0 {
		steps = append(steps, &Redact{Keys: cfg.Redact})
	}
	for _, s := range steps {
		if r, ok := s.(*Redact); ok {
			if _, err := r.regexes(); err != nil {
				return nil, &ConfigError{Msg: err.Error(), Err: err}
			}
		}
	}
	named := map[string]Alarm{}
	var fallback Alarm
	for n, a := range cfg.Alarms {
		if n == "panic" || n == "ignore" {
			return nil, configErr("`%s` cannot name an alarm", n)
		}
		if t, ok := a.(*Tiers); ok {
			if err := t.check(); err != nil {
				return nil, &ConfigError{Msg: fmt.Sprintf("alarm `%s`: %v", n, err), Err: err}
			}
		}
		if n == "default" {
			fallback = a
		} else {
			named[n] = a
		}
	}
	sinks := cfg.Sinks
	if len(sinks) == 0 {
		c, err := console()
		if err != nil {
			return nil, err
		}
		sinks = []Sink{c}
	}
	ls, err := prepare(sinks, alongside, named)
	if err != nil {
		return nil, err
	}
	ok := known(ls)
	check := func(whose string, a Alarm) error {
		if a == nil || a == defaultAlarm {
			return nil
		}
		for _, n := range names(a) {
			if !ok[n] {
				return configErr("%s routes to `%s`, which is not a sink", whose, n)
			}
		}
		return nil
	}
	if err := check("the default alarm", fallback); err != nil {
		return nil, err
	}
	for _, l := range ls {
		if err := check(fmt.Sprintf("the alarm of `%s`", l.name()), l.core.alarm); err != nil {
			return nil, err
		}
	}
	m := detectMeta()
	return &plan{
		meta:   Meta{or(cfg.App, m.App), or(cfg.Env, m.Env), or(cfg.Instance, m.Instance)},
		compat: profile, filter: f, steps: steps, live: ls, alarm: fallback, named: named,
	}, nil
}

func apply(p *plan, swapAll bool) {
	if p.alarm != nil {
		SetAlarm(p.alarm)
	}
	nameAlarms(p.named)
	var old []*live
	change(func(s *state) {
		floor, ok := p.filter.Floor()
		s.meta, s.compat, s.filter, s.floor, s.off, s.steps = p.meta, p.compat, p.filter, floor, !ok, p.steps
		if swapAll {
			old = s.table
			swap(s, p.live)
		} else {
			swap(s, slices.Concat(s.table, p.live))
		}
	})
	retire(old)
}
