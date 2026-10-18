package serlog

import (
	"cmp"
	"fmt"
	"maps"
	"regexp"
	"slices"
	"strings"
	"sync"
	"time"
)

// Processor is a step a record goes through before it is written: return it, a changed copy, or nil to drop it.
// Change a copy (Record.Clone or Record.With), never the record itself: other slots still see the original.
type Processor interface {
	Process(r *Record) *Record
}

// ProcessorFunc is a function used as a Processor.
type ProcessorFunc func(r *Record) *Record

// Process calls f.
func (f ProcessorFunc) Process(r *Record) *Record { return f(r) }

// exporter is anything that can describe itself as a config entry.
type exporter interface{ Options() map[string]any }

//go:noinline
func runSteps(steps []Processor, r *Record) *Record {
	active.Add(1)
	defer active.Add(-1)
	for _, s := range steps {
		if r = s.Process(r); r == nil {
			return nil
		}
	}
	return r
}

// Redact masks fields by name at any depth or by dotted path, and parts of the message by regex.
type Redact struct {
	Keys     []string
	Paths    []string
	Patterns []string
	Mask     string
	once     sync.Once
	res      []*regexp.Regexp
	err      error
}

func (r *Redact) regexes() ([]*regexp.Regexp, error) {
	r.once.Do(func() {
		for _, p := range r.Patterns {
			re, err := regexp.Compile(p)
			if err != nil {
				r.res, r.err = nil, fmt.Errorf("redact: %w", err)
				return
			}
			r.res = append(r.res, re)
		}
	})
	return r.res, r.err
}

func (r *Redact) mask() string { return or(r.Mask, "***") }

func (r *Redact) scrub(path, key string, v any) (any, bool) {
	if slices.Contains(r.Keys, key) || slices.Contains(r.Paths, path) {
		return r.mask(), true
	}
	switch x := v.(type) {
	case Group:
		out, changed := make(Group, len(x)), false
		for i, f := range x {
			nv, c := r.scrub(path+"."+f.Key, f.Key, f.Value)
			out[i], changed = Field{f.Key, nv}, changed || c
		}
		if changed {
			return out, true
		}
	case map[string]any:
		out, changed := make(map[string]any, len(x)), false
		for k, val := range x {
			nv, c := r.scrub(path+"."+k, k, val)
			out[k], changed = nv, changed || c
		}
		if changed {
			return out, true
		}
	}
	return v, false
}

// Process masks what it should and copies the record only when something changed.
func (r *Redact) Process(rec *Record) *Record {
	res, _ := r.regexes()
	msg := rec.Msg
	for _, re := range res {
		msg = re.ReplaceAllLiteralString(msg, r.mask())
	}
	var fields []Field
	for i, f := range rec.Fields {
		if nv, c := r.scrub(f.Key, f.Key, f.Value); c {
			if fields == nil {
				fields = slices.Clone(rec.Fields)
			}
			fields[i] = Field{f.Key, nv}
		}
	}
	if fields == nil && msg == rec.Msg {
		return rec
	}
	out := *rec
	out.Msg = msg
	if fields != nil {
		out.Fields = fields
	}
	return &out
}

// Options describes the step for export.
func (r *Redact) Options() map[string]any {
	o := map[string]any{}
	if len(r.Keys) > 0 {
		o["keys"] = slices.Sorted(slices.Values(r.Keys))
	}
	if len(r.Paths) > 0 {
		o["paths"] = r.Paths
	}
	if len(r.Patterns) > 0 {
		o["patterns"] = r.Patterns
	}
	if r.Mask != "" && r.Mask != "***" {
		o["mask"] = r.Mask
	}
	return map[string]any{"redact": o}
}

// Sanitise escapes line breaks and control characters in the message and string fields, against log forging.
type Sanitise struct{}

func escaped(s string) string {
	if !strings.ContainsFunc(s, func(c rune) bool { return c < 0x20 || c == 0x7f }) {
		return s
	}
	var b strings.Builder
	for _, c := range s {
		switch {
		case c == '\n':
			b.WriteString(`\n`)
		case c == '\r':
			b.WriteString(`\r`)
		case c == '\t':
			b.WriteString(`\t`)
		case c < 0x20 || c == 0x7f:
			fmt.Fprintf(&b, `\u{%04x}`, c)
		default:
			b.WriteRune(c)
		}
	}
	return b.String()
}

// Process escapes what it should and copies the record only when something changed.
func (Sanitise) Process(rec *Record) *Record {
	msg := escaped(rec.Msg)
	var fields []Field
	for i, f := range rec.Fields {
		if s, ok := f.Value.(string); ok {
			if c := escaped(s); c != s {
				if fields == nil {
					fields = slices.Clone(rec.Fields)
				}
				fields[i] = Field{f.Key, c}
			}
		}
	}
	if fields == nil && msg == rec.Msg {
		return rec
	}
	out := *rec
	out.Msg = msg
	if fields != nil {
		out.Fields = fields
	}
	return &out
}

// Options describes the step for export.
func (Sanitise) Options() map[string]any { return map[string]any{"sanitise": true} }

// Fields sets the same fields on every record, in key order.
type Fields map[string]any

// Process sets the fields.
func (f Fields) Process(rec *Record) *Record {
	if len(f) == 0 {
		return rec
	}
	set := make([]Field, 0, len(f))
	for _, k := range slices.Sorted(maps.Keys(f)) {
		set = append(set, Field{k, f[k]})
	}
	return rec.With(set...)
}

// Options describes the step for export.
func (f Fields) Options() map[string]any {
	return map[string]any{"fields": map[string]any(maps.Clone(f))}
}

// Match keeps a record only when every condition given holds: tags, fields compared as text, a lowest level.
// With Exclude it drops the record then instead.
type Match struct {
	Tags    []string
	Fields  map[string]any
	Level   Level
	Exclude bool
}

// Holds is whether every condition holds.
func (m *Match) Holds(r *Record) bool {
	if m.Level != 0 && r.Level < m.Level {
		return false
	}
	for _, t := range m.Tags {
		if !slices.Contains(r.Tags, t) {
			return false
		}
	}
	for k, want := range m.Fields {
		v, ok := r.Field(k)
		if !ok || show(v) != show(want) {
			return false
		}
	}
	return true
}

// Process keeps or drops the record.
func (m *Match) Process(r *Record) *Record {
	if m.Holds(r) != m.Exclude {
		return r
	}
	return nil
}

// Options describes the step for export.
func (m *Match) Options() map[string]any {
	o := map[string]any{}
	if len(m.Tags) > 0 {
		o["tags"] = m.Tags
	}
	if len(m.Fields) > 0 {
		f := map[string]any{}
		for k, v := range m.Fields {
			f[k] = show(v)
		}
		o["fields"] = f
	}
	if m.Level != 0 {
		o["level"] = m.Level.String()
	}
	name := "match"
	if m.Exclude {
		name = "exclude"
	}
	return map[string]any{name: o}
}

// Sample keeps, per level and message, the first First in each Per, then one in Every.
type Sample struct {
	First int
	Every int
	Per   time.Duration
	mu    sync.Mutex
	seen  map[string]*count
}

type count struct {
	at time.Time
	n  int
}

// Process keeps or drops the record.
func (s *Sample) Process(r *Record) *Record {
	s.mu.Lock()
	defer s.mu.Unlock()
	now, per := time.Now(), cmp.Or(s.Per, time.Second)
	if s.seen == nil || len(s.seen) > 10_000 {
		s.seen = map[string]*count{}
	}
	key := fmt.Sprint(r.Level, "\x00", r.Msg)
	c, ok := s.seen[key]
	if !ok || now.Sub(c.at) >= per {
		c = &count{at: now}
		s.seen[key] = c
	}
	c.n++
	if c.n <= s.First || (c.n-s.First)%max(s.Every, 1) == 0 {
		return r
	}
	return nil
}

// Options describes the step for export.
func (s *Sample) Options() map[string]any {
	return map[string]any{"sample": map[string]any{"first": s.First, "every": max(s.Every, 1), "per": showTime(cmp.Or(s.Per, time.Second))}}
}

// Limit is a token bucket: Rate records a second, up to Burst at once.
type Limit struct {
	Rate   float64
	Burst  float64
	mu     sync.Mutex
	tokens float64
	last   time.Time
}

func (l *Limit) burst() float64 { return max(cmp.Or(l.Burst, l.Rate), 1) }

// Process keeps the record while tokens last.
func (l *Limit) Process(r *Record) *Record {
	l.mu.Lock()
	defer l.mu.Unlock()
	now := time.Now()
	if l.last.IsZero() {
		l.tokens = l.burst()
	} else {
		l.tokens = min(l.burst(), l.tokens+now.Sub(l.last).Seconds()*l.Rate)
	}
	l.last = now
	if l.tokens < 1 {
		return nil
	}
	l.tokens--
	return r
}

// Options describes the step for export.
func (l *Limit) Options() map[string]any {
	return map[string]any{"limit": map[string]any{"rate": l.Rate, "burst": l.burst()}}
}

// Dedupe drops a record repeated within Within; the next one kept carries `repeated`, the number dropped.
type Dedupe struct {
	Within time.Duration
	mu     sync.Mutex
	seen   map[string]*count
}

// Process keeps or drops the record.
func (d *Dedupe) Process(r *Record) *Record {
	d.mu.Lock()
	defer d.mu.Unlock()
	now, within := time.Now(), cmp.Or(d.Within, time.Second)
	if d.seen == nil || len(d.seen) > 10_000 {
		d.seen = map[string]*count{}
	}
	key := fmt.Sprint(r.Level, "\x00", r.Target, "\x00", r.Msg)
	c, ok := d.seen[key]
	if ok && now.Sub(c.at) < within {
		c.n++
		return nil
	}
	d.seen[key] = &count{at: now}
	if ok && c.n > 0 {
		return r.With(Field{"repeated", c.n})
	}
	return r
}

// Options describes the step for export.
func (d *Dedupe) Options() map[string]any {
	return map[string]any{"dedupe": map[string]any{"within": showTime(cmp.Or(d.Within, time.Second))}}
}
