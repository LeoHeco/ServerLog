package serlog

import (
	"fmt"
	"sync"
)

// The four kinds of extension a config file can name: sinks (`kind`), processors (a `process` entry), layouts
// (`layout: name`) and alarms (`kind` under `alarms`). A factory receives the entry's own keys and must read them
// all.
var (
	kindsMu   sync.RWMutex
	sinkKinds = map[string]func(*Options) (Sink, error){
		"console": func(o *Options) (Sink, error) { return &Console{Stream: o.String("stream")}, nil },
		"file":    fileKind,
		"loki": func(o *Options) (Sink, error) {
			return &Loki{URL: o.Need("url"), Labels: o.Table("labels"), Timeout: o.Duration("timeout"), Connect: o.Bool("connect")}, nil
		},
		"mail":   mailKind,
		"memory": func(o *Options) (Sink, error) { return &Memory{Size: o.Int("size")}, nil },
	}
	stepKinds = map[string]func(*Options) (Processor, error){
		"redact": func(o *Options) (Processor, error) {
			r := &Redact{Keys: o.Strings("keys"), Paths: o.Strings("paths"), Patterns: o.Strings("patterns"), Mask: o.String("mask")}
			_, err := r.regexes()
			return r, err
		},
		"sanitise": func(*Options) (Processor, error) { return Sanitise{}, nil },
		"fields":   func(o *Options) (Processor, error) { return Fields(o.All()), nil },
		"match":    func(o *Options) (Processor, error) { return matchKind(o, false) },
		"exclude":  func(o *Options) (Processor, error) { return matchKind(o, true) },
		"sample": func(o *Options) (Processor, error) {
			first, every := 100, 100
			if o.Has("first") {
				first = o.Int("first")
			}
			if o.Has("every") {
				every = o.Int("every")
			}
			return &Sample{First: first, Every: every, Per: o.Duration("per")}, nil
		},
		"limit": func(o *Options) (Processor, error) {
			if !o.Has("rate") {
				return nil, configErr("limit: `rate` is required")
			}
			return &Limit{Rate: o.Float("rate"), Burst: o.Float("burst")}, nil
		},
		"dedupe": func(o *Options) (Processor, error) { return &Dedupe{Within: o.Duration("within")}, nil },
	}
	layoutKinds = map[string]func() Layout{}
	alarmKinds  = map[string]func(*Options) (Alarm, error){
		"tiers": func(o *Options) (Alarm, error) {
			t := &Tiers{OnFail: o.String("on_fail"), Remind: o.Duration("remind")}
			if v := o.Raw("tiers"); v != nil {
				tiers, ok := v.([]any)
				if !ok {
					return nil, configErr("`tiers` takes a list of lists of sink names")
				}
				for _, tier := range tiers {
					switch x := tier.(type) {
					case []any:
						names := make([]string, len(x))
						for i, n := range x {
							names[i] = fmt.Sprint(n)
						}
						t.Tiers = append(t.Tiers, names)
					default:
						t.Tiers = append(t.Tiers, []string{fmt.Sprint(x)})
					}
				}
			}
			return t, t.check()
		},
	}
)

func fileKind(o *Options) (Sink, error) {
	f := &File{
		Path: o.Need("path"), MaxSize: o.Size("max_size"), Rotate: o.String("rotate"), Keep: o.Int("keep"),
		MaxAge: o.Duration("max_age"), MaxTotal: o.Size("max_total"), Gzip: o.Bool("gzip"),
		RollName: o.String("roll_name"), Reopen: o.Bool("reopen"), Buffer: o.Size("buffer"),
		FlushEvery: o.Duration("flush_every"),
	}
	f.Truncate = o.Has("append") && !o.Bool("append")
	f.NoMkdir = o.Has("mkdir") && !o.Bool("mkdir")
	switch p := o.Raw("perms").(type) {
	case nil:
	case string:
		f.Perms = p
	default:
		if n, ok := toFloat(p); ok {
			f.Perms = fmt.Sprintf("%04o", int(n))
		}
	}
	return f, nil
}

func mailKind(o *Options) (Sink, error) {
	m := &Mail{
		Host: o.Need("host"), Port: o.Int("port"), Security: o.String("security"), User: o.String("user"),
		To: o.Strings("to"), Timeout: o.Duration("timeout"), Connect: o.Bool("connect"),
	}
	m.Pass = or(o.String("pass"), o.String("password"))
	m.From = or(o.String("from"), o.String("sender"))
	return m, nil
}

func matchKind(o *Options, exclude bool) (Processor, error) {
	m := &Match{Tags: o.Strings("tags"), Exclude: exclude}
	if v := o.Raw("fields"); v != nil {
		f, ok := v.(map[string]any)
		if !ok {
			return nil, configErr("match: `fields` takes a map")
		}
		m.Fields = f
	}
	if v := o.String("level"); v != "" {
		l, err := ParseLevel(v)
		if err != nil {
			return nil, &ConfigError{Msg: err.Error(), Err: err}
		}
		m.Level = l
	}
	return m, nil
}

// RegisterSink lets config files name a kind of sink.
func RegisterSink(kind string, fn func(*Options) (Sink, error)) {
	kindsMu.Lock()
	defer kindsMu.Unlock()
	sinkKinds[kind] = fn
}

// RegisterProcessor lets config files name a processor in `process`.
func RegisterProcessor(name string, fn func(*Options) (Processor, error)) {
	kindsMu.Lock()
	defer kindsMu.Unlock()
	stepKinds[name] = fn
}

// RegisterLayout lets config files name a layout with `layout: name`.
func RegisterLayout(name string, fn func() Layout) {
	kindsMu.Lock()
	defer kindsMu.Unlock()
	layoutKinds[name] = fn
}

// RegisterAlarm lets config files name a kind of alarm with `kind: name` under `alarms`.
func RegisterAlarm(kind string, fn func(*Options) (Alarm, error)) {
	kindsMu.Lock()
	defer kindsMu.Unlock()
	alarmKinds[kind] = fn
}

func lookupKind[T any](table map[string]T, what, name string) (T, error) {
	kindsMu.RLock()
	defer kindsMu.RUnlock()
	k, ok := table[name]
	if !ok {
		var zero T
		return zero, &KindError{what, name}
	}
	return k, nil
}

func layoutKind(name string) (Layout, error) {
	fn, err := lookupKind(layoutKinds, "layout", name)
	if err != nil {
		return nil, err
	}
	return fn(), nil
}

func build[T any](what string, fn func(*Options) (T, error), keys map[string]any) (T, error) {
	o := newOptions(what, keys)
	v, err := fn(o)
	if err == nil {
		err = o.done()
	}
	if err != nil {
		var zero T
		if _, ok := err.(*ConfigError); !ok {
			err = &ConfigError{Msg: fmt.Sprintf("%s: %v", what, err), Err: err}
		}
		return zero, err
	}
	return v, nil
}
