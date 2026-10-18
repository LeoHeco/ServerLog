package serlog

import (
	"context"
	"time"
)

// Sink is somewhere entries go. Only Emit is required: queues, retries, health, alarms and layout come from the
// slot, so a sink only delivers and returns what went wrong; wrap ErrRejected when sending again will not help.
// Calls into one sink are serialised.
//
// A sink may also implement EmitBatch([]*Entry) error, Enabled(Level, string) bool, Flush() error, Ready() error,
// Claim() string, Kind() string, TTY() bool, DefaultLayout() Layout, DefaultQueue() Queue and
// Options() map[string]any; embedding Slot gives it slot settings of its own.
type Sink interface {
	Emit(e *Entry) error
}

type (
	batcher interface{ EmitBatch(es []*Entry) error }
	enabler interface {
		Enabled(l Level, target string) bool
	}
	flusher     interface{ Flush() error }
	readier     interface{ Ready() error }
	claimer     interface{ Claim() string }
	kinder      interface{ Kind() string }
	ttyer       interface{ TTY() bool }
	defLayout   interface{ DefaultLayout() Layout }
	defQueue    interface{ DefaultQueue() Queue }
	slotCarrier interface{ SlotKeys() *Slot }
)

// SinkFunc is a function used as a Sink.
type SinkFunc func(e *Entry) error

// Emit calls f.
func (f SinkFunc) Emit(e *Entry) error { return f(e) }

// Kind is "fn".
func (SinkFunc) Kind() string { return "fn" }

// Queue is the shape of a queue in front of a sink: it holds Size records, written Batch at a time or Wait after
// the first arrives; Full is "drop" or "block".
type Queue struct {
	Size  int
	Batch int
	Wait  time.Duration
	Full  string
}

// The sentinels for Slot fields where zero means the default.
const (
	// Direct, as Slot.Queue, writes without a queue.
	Direct = -1
	// NoRetries, as Slot.Retries, never tries again.
	NoRetries = -1
)

// Slot holds the settings of the slot a sink goes into: the keys of a config entry. Zero values mean the
// default. Embed it in a sink to give the sink settings of its own.
type Slot struct {
	Name       string
	Level      Level
	Filter     string
	Process    []Processor
	Layout     Layout
	LayoutName string
	Format     string
	Pattern    string
	Schema     string
	Keys       map[string]string
	Levels     string
	Nest       string
	Colour     string
	Colours    Theme
	AlarmOnly  bool
	OnFail     string
	Alarm      Alarm
	Queue      int
	Batch      int
	Wait       time.Duration
	Full       string
	Retries    int
	Probe      time.Duration
}

// SlotKeys gives the settings to whatever embeds them.
func (s *Slot) SlotKeys() *Slot { return s }

func merge(base, over Slot) Slot {
	set := func(dst *string, v string) {
		if v != "" {
			*dst = v
		}
	}
	set(&base.Name, over.Name)
	set(&base.Filter, over.Filter)
	set(&base.LayoutName, over.LayoutName)
	set(&base.Format, over.Format)
	set(&base.Pattern, over.Pattern)
	set(&base.Schema, over.Schema)
	set(&base.Levels, over.Levels)
	set(&base.Nest, over.Nest)
	set(&base.Colour, over.Colour)
	set(&base.OnFail, over.OnFail)
	set(&base.Full, over.Full)
	if over.Level != 0 {
		base.Level = over.Level
	}
	if over.Process != nil {
		base.Process = over.Process
	}
	if over.Layout != nil {
		base.Layout = over.Layout
	}
	if over.Keys != nil {
		base.Keys = over.Keys
	}
	if over.Colours != nil {
		base.Colours = over.Colours
	}
	base.AlarmOnly = base.AlarmOnly || over.AlarmOnly
	if over.Alarm != nil {
		base.Alarm = over.Alarm
	}
	if over.Queue != 0 {
		base.Queue = over.Queue
	}
	if over.Batch != 0 {
		base.Batch = over.Batch
	}
	if over.Wait != 0 {
		base.Wait = over.Wait
	}
	if over.Retries != 0 {
		base.Retries = over.Retries
	}
	if over.Probe != 0 {
		base.Probe = over.Probe
	}
	return base
}

// slotted is a sink with slot settings laid over its own.
type slotted struct {
	Sink
	slot Slot
}

// With is sink in a slot with these settings, laid over those the sink carries; a sink that embeds Slot gets them
// written into it.
func With(sink Sink, slot Slot) Sink {
	if c, ok := sink.(slotCarrier); ok {
		*c.SlotKeys() = merge(*c.SlotKeys(), slot)
		return sink
	}
	return &slotted{Sink: sink, slot: merge(slotOf(sink), slot)}
}

func slotOf(s Sink) Slot {
	switch x := s.(type) {
	case *slotted:
		return x.slot
	case slotCarrier:
		return *x.SlotKeys()
	}
	return Slot{}
}

func inner(s Sink) Sink {
	if x, ok := s.(*slotted); ok {
		return x.Sink
	}
	return s
}

// Entry is what a sink is handed: the record, and the line its slot's layout makes of it, laid out on first use
// and shared with other slots using the same layout.
type Entry struct {
	Record *Record
	layout Layout
	theme  *Theme
	cache  map[cacheKey]string
	line   string
	done   bool
	owner  *core
}

type cacheKey struct {
	layout Layout
	theme  *Theme
}

// NewEntry is an uncoloured entry laid out by l.
func NewEntry(r *Record, l Layout) *Entry { return &Entry{Record: r, layout: l} }

// Line is the line.
func (e *Entry) Line() string {
	if !e.done {
		k := cacheKey{e.layout, e.theme}
		if s, ok := e.cache[k]; ok {
			e.line = s
		} else {
			var t Theme
			if e.theme != nil {
				t = *e.theme
			}
			e.line = e.layout.Render(e.Record, t)
			if e.cache != nil {
				e.cache[k] = e.line
			}
		}
		e.done = true
	}
	return e.line
}

// Coloured is whether the line is coloured.
func (e *Entry) Coloured() bool { return e.theme != nil }

// Context marks code as serlog's own: log with it inside a sink, and what is logged is dropped instead of
// looping back.
func (e *Entry) Context() context.Context {
	return context.WithValue(context.Background(), guardKey{}, e.owner)
}
