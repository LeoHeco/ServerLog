package serlog

import (
	"context"
	"fmt"
	"slices"
	"sync"
	"sync/atomic"
	"time"
)

// state is everything the hot path reads, swapped whole when anything changes.
type state struct {
	filter     *Filter
	floor      Level
	off        bool
	steps      []Processor
	table      []*live
	configured bool
	caller     bool
	compat     Compat
	meta       Meta
}

var (
	edit     sync.Mutex
	current  atomic.Pointer[state]
	console0 = sync.OnceValue(func() *live {
		l, _ := newLive(&Console{}, "console", nil)
		return l
	})
	stderr0 = sync.OnceValue(func() *live {
		l, _ := newLive(&Console{Slot: Slot{OnFail: "ignore"}, Stream: "stderr"}, "stderr", nil)
		return l
	})
	alarm0 atomic.Pointer[Alarm]
)

func init() {
	current.Store(&state{filter: LevelFilter(LevelInfo), floor: LevelInfo, compat: CompatSerlog, meta: detectMeta()})
}

func load() *state { return current.Load() }

// change swaps in a copy of the state with fn applied.
func change(fn func(s *state)) {
	edit.Lock()
	defer edit.Unlock()
	s := *load()
	fn(&s)
	current.Store(&s)
}

func compat() Compat { return load().compat }
func meta() Meta     { return load().meta }

// Enabled is whether a record at level could pass the filter for some target.
func Enabled(l Level) bool {
	s := load()
	return !s.off && l >= s.floor
}

// SetFilter replaces the global filter; it takes effect at once.
func SetFilter(f *Filter) {
	change(func(s *state) {
		floor, ok := f.Floor()
		s.filter, s.floor, s.off = f, floor, !ok
	})
}

// SetLevel lets level and above through for every target.
func SetLevel(l Level) { SetFilter(LevelFilter(l)) }

// GetFilter is the global filter in force.
func GetFilter() *Filter { return load().filter }

// SetProcess replaces the global processor chain.
func SetProcess(steps ...Processor) { change(func(s *state) { s.steps = slices.Clone(steps) }) }

// AddProcess adds a step to the end of the global chain.
func AddProcess(p Processor) {
	change(func(s *state) { s.steps = append(slices.Clone(s.steps), p) })
}

// RedactKeys masks fields with these names, wherever they appear.
func RedactKeys(keys ...string) { AddProcess(&Redact{Keys: keys}) }

type currentAlarm struct{}

func cur() Alarm {
	if a := alarm0.Load(); a != nil {
		return *a
	}
	return &Tiers{}
}

func (currentAlarm) Route(i *Incident) [][]string      { return cur().Route(i) }
func (currentAlarm) Describe(i *Incident) *Record      { return describe(cur(), i) }
func (currentAlarm) Settings() (string, time.Duration) { return settings(cur()) }
func (currentAlarm) Names() []string                   { return names(cur()) }

// defaultAlarm is what every slot without an on_fail reports to: the default alarm in force at the time.
var defaultAlarm Alarm = currentAlarm{}

func resolve(a Alarm) Alarm {
	if _, ok := a.(currentAlarm); ok {
		return cur()
	}
	return a
}

// SetAlarm replaces the default alarm.
func SetAlarm(a Alarm) { alarm0.Store(&a) }

// emit sends a record through the filter, the scope and the global processors, then to every slot on the log
// route.
func emit(ctx context.Context, r *Record) {
	if suppress(ctx) {
		return
	}
	s := load()
	if !s.filter.Allows(r.Level, r.Target) || s.filter.pattern != nil && !s.filter.Admits(r.Msg) {
		return
	}
	if len(s.steps) > 0 {
		if r = runSteps(s.steps, r); r == nil {
			return
		}
	}
	dispatch(s, r)
}

// Emit sends a record made elsewhere through the filter and the processors.
func Emit(r *Record) { emit(context.Background(), r) }

func dispatch(s *state, r *Record) {
	if !s.configured {
		console0().emit(r, nil)
		return
	}
	var cache map[cacheKey]string
	if len(s.table) > 1 {
		cache = map[cacheKey]string{}
	}
	for _, l := range s.table {
		if !l.alarmOnly {
			l.emit(r, cache)
		}
	}
}

// prepare names, checks and wraps new sinks against those already registered; nothing is kept on error.
func prepare(sinks []Sink, existing []*live, named map[string]Alarm) ([]*live, error) {
	names := map[string]bool{"*": true}
	claims := map[string]bool{}
	counts := map[string]int{}
	for _, l := range existing {
		names[l.name()] = true
		if l.claim != "" {
			claims[l.claim] = true
		}
		counts[l.core.kind]++
	}
	var out []*live
	for _, s := range sinks {
		kind := kindOf(s)
		name := slotOf(s).Name
		for name == "" {
			counts[kind]++
			if n := fmt.Sprintf("%s-%d", kind, counts[kind]); !names[n] {
				name = n
			}
		}
		if names[name] {
			return nil, &NameTakenError{name}
		}
		names[name] = true
		l, err := newLive(s, name, named)
		if err != nil {
			return nil, err
		}
		if l.claim != "" {
			if claims[l.claim] {
				return nil, &ClaimedError{l.claim}
			}
			claims[l.claim] = true
		}
		if r, ok := inner(s).(readier); ok {
			if err := r.Ready(); err != nil {
				return nil, &NotReadyError{name, err}
			}
		}
		out = append(out, l)
	}
	return out, nil
}

func known(extra []*live) map[string]bool {
	out := map[string]bool{"stderr": true, "*": true}
	for _, l := range slices.Concat(load().table, extra) {
		out[l.name()] = true
	}
	return out
}

func swap(s *state, table []*live) {
	s.table = table
	s.configured = true
	s.caller = slices.ContainsFunc(table, func(l *live) bool { return l.caller })
}

// Add registers a sink at run time and returns the name it got.
func Add(sink Sink) (string, error) {
	edit.Lock()
	defer edit.Unlock()
	s := *load()
	ls, err := prepare([]Sink{sink}, s.table, nil)
	if err != nil {
		return "", err
	}
	swap(&s, slices.Concat(s.table, ls))
	current.Store(&s)
	return ls[0].name(), nil
}

// Remove takes a slot out, writes what its queue holds, stops its goroutine and closes its sink if it is an
// io.Closer. False when no slot has that name.
func Remove(name string) bool {
	var gone []*live
	change(func(s *state) {
		s.table = slices.DeleteFunc(slices.Clone(s.table), func(l *live) bool {
			if l.name() == name {
				gone = append(gone, l)
				return true
			}
			return false
		})
	})
	retire(gone)
	return len(gone) > 0
}

func retire(ls []*live) {
	for _, l := range ls {
		l.retire()
	}
}

func find(name string) *live {
	for _, l := range load().table {
		if l.name() == name {
			return l
		}
	}
	if name == "stderr" {
		return stderr0()
	}
	return nil
}

// Statuses is every slot's state.
func Statuses() []Status {
	t := load().table
	out := make([]Status, len(t))
	for i, l := range t {
		out[i] = l.core.status()
	}
	return out
}

// Flush waits up to five seconds for every queue and then the alarm route to drain. Call it before the program
// ends, as `defer serlog.Flush()`.
func Flush() { FlushWithin(5 * time.Second) }

// FlushWithin waits up to within for every queue and then the alarm route to drain.
func FlushWithin(within time.Duration) {
	until := time.Now().Add(within)
	flushSlots(within)
	settle(time.Until(until))
}

func flushSlots(within time.Duration) {
	until := time.Now().Add(within)
	for _, l := range load().table {
		l.flush(max(time.Until(until), 0))
	}
}

// board is the registered slots, as the alarm walk sees them.
type board struct{}

func (board) logs() []string {
	var out []string
	for _, l := range load().table {
		if !l.alarmOnly {
			out = append(out, l.name())
		}
	}
	return out
}

func (board) resting(name string) bool {
	l := find(name)
	return l != nil && l.core.resting()
}

func (board) deliver(name string, r *Record) error {
	l := find(name)
	if l == nil {
		return fmt.Errorf("no sink called `%s`", name)
	}
	return l.core.deliver(r)
}
