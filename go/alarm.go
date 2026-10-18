package serlog

import (
	"cmp"
	"fmt"
	"os"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// Alarm routes incidents to sinks: Route gives the tiers, tried in order, `*` standing for every sink on the log
// route. The alarm task leaves out the failing sink and any that are resting, delivers within a tier at once and
// stops at the first tier where one sink took it, so an alarm cannot cause loops or storms.
//
// An alarm may also implement Describe(*Incident) *Record for its own record, Settings() (onFail string, remind
// time.Duration), Names() []string for the start-up check, and Options() map[string]any for export. Tiers does
// all of these.
type Alarm interface {
	Route(i *Incident) [][]string
}

type (
	describer interface{ Describe(i *Incident) *Record }
	settler   interface {
		Settings() (onFail string, remind time.Duration)
	}
	namer interface{ Names() []string }
)

// Incident is a sink's failure, from the first error to recovery. The walk adds every failed delivery to
// Failures.
type Incident struct {
	Sink     string
	Kind     string
	Phase    Phase
	Error    string
	Failures []Failure
	Skipped  int64
	Dropped  int64
	Since    time.Time
}

// Failure is one failed write or delivery.
type Failure struct {
	Sink  string
	Error string
	Time  time.Time
}

// NewIncident is a fresh incident; its failures start with the sink's own unless it is resolved.
func NewIncident(sink, kind string, phase Phase, err string) *Incident {
	i := &Incident{Sink: sink, Kind: kind, Phase: phase, Error: err, Since: time.Now()}
	if phase != Resolved {
		i.Failures = []Failure{{sink, err, time.Now()}}
	}
	return i
}

// Describe is the default record for an incident.
func Describe(i *Incident) *Record {
	level, msg := LevelError, fmt.Sprintf("sink `%s` failed: %s", i.Sink, i.Error)
	switch i.Phase {
	case Reminder:
		msg = fmt.Sprintf("sink `%s` is still failing: %s", i.Sink, i.Error)
	case Resolved:
		level, msg = LevelInfo, fmt.Sprintf("sink `%s` works again after %ds", i.Sink, int(time.Since(i.Since).Seconds()))
	}
	fails := make([]string, len(i.Failures))
	for n, f := range i.Failures {
		fails[n] = f.Sink + ": " + f.Error
	}
	return NewRecord(level, "serlog", msg,
		Field{"sink", i.Sink}, Field{"kind", i.Kind}, Field{"phase", string(i.Phase)},
		Field{"failures", strings.Join(fails, "; ")}, Field{"skipped", i.Skipped}, Field{"dropped", i.Dropped},
		Field{"since", iso(i.Since, 9)})
}

// Tiers is the built-in alarm: fixed tiers of sink names. OnFail is "ignore" (the default) or "panic", for when
// no tier took the incident; Remind is how long a sink may stay down before it is reported again, an hour by
// default.
type Tiers struct {
	Tiers  [][]string
	OnFail string
	Remind time.Duration
}

// Route gives the tiers, `[[stderr]]` when there are none.
func (t *Tiers) Route(*Incident) [][]string {
	if len(t.Tiers) == 0 {
		return [][]string{{"stderr"}}
	}
	return t.Tiers
}

// Settings gives the fallback and the reminder interval.
func (t *Tiers) Settings() (string, time.Duration) {
	return or(t.OnFail, "ignore"), cmp.Or(t.Remind, time.Hour)
}

// Names lists every sink it may route to.
func (t *Tiers) Names() []string { return slices.Concat(t.Route(nil)...) }

// Options describes the alarm for export.
func (t *Tiers) Options() map[string]any {
	o := map[string]any{"tiers": t.Route(nil)}
	if t.OnFail == "panic" {
		o["on_fail"] = "panic"
	}
	if r := cmp.Or(t.Remind, time.Hour); r != time.Hour {
		o["remind"] = showTime(r)
	}
	return o
}

func (t *Tiers) check() error {
	for _, tier := range t.Tiers {
		if len(tier) == 0 {
			return fmt.Errorf("`tiers` needs at least one non-empty tier")
		}
	}
	if t.OnFail != "" && t.OnFail != "panic" && t.OnFail != "ignore" {
		return fmt.Errorf("an alarm's `on_fail` is panic or ignore, not `%s`", t.OnFail)
	}
	return nil
}

func settings(a Alarm) (string, time.Duration) {
	if s, ok := a.(settler); ok {
		return s.Settings()
	}
	return "ignore", time.Hour
}

func describe(a Alarm, i *Incident) *Record {
	if d, ok := a.(describer); ok {
		return d.Describe(i)
	}
	return Describe(i)
}

func names(a Alarm) []string {
	if n, ok := a.(namer); ok {
		return n.Names()
	}
	return nil
}

// desk is what the walk needs from the sinks around it.
type desk interface {
	logs() []string
	resting(name string) bool
	deliver(name string, r *Record) error
}

// sound delivers the incident through the alarm's tiers, adding every failed delivery to it; true when one sink
// took it.
func sound(a Alarm, i *Incident, d desk) bool {
	out := map[string]bool{i.Sink: true}
	for _, tier := range a.Route(i) {
		var targets []string
		for _, n := range tier {
			expand := []string{n}
			if n == "*" {
				expand = d.logs()
			}
			for _, m := range expand {
				if !out[m] && !d.resting(m) && !slices.Contains(targets, m) {
					targets = append(targets, m)
				}
			}
		}
		if len(targets) == 0 {
			continue
		}
		rec := describe(a, i)
		errs := make([]error, len(targets))
		var wg sync.WaitGroup
		for n, name := range targets {
			wg.Go(func() { errs[n] = d.deliver(name, rec) })
		}
		wg.Wait()
		heard := false
		for n, name := range targets {
			if errs[n] == nil {
				heard = true
			} else {
				i.Failures = append(i.Failures, Failure{name, message(errs[n]), time.Now()})
			}
			out[name] = true
		}
		if heard {
			return true
		}
	}
	return false
}

type job struct {
	alarm    Alarm
	incident *Incident
}

var (
	jobs      = make(chan job, 64)
	jobsStart sync.Once
	pending   atomic.Int64
	lost      atomic.Int64
	crashing  atomic.Bool
)

func post(a Alarm, i *Incident) {
	jobsStart.Do(func() { go drainAlarms() })
	pending.Add(1)
	select {
	case jobs <- job{resolve(a), i}:
	default:
		pending.Add(-1)
		lost.Add(1)
	}
}

func drainAlarms() {
	for j := range jobs {
		heard := sound(j.alarm, j.incident, board{})
		if onFail, _ := settings(j.alarm); !heard && onFail == "panic" {
			crash(fmt.Sprintf("no sink took the alarm for `%s`: %s", j.incident.Sink, j.incident.Error))
		}
		pending.Add(-1)
	}
}

func settle(within time.Duration) {
	until := time.Now().Add(within)
	for pending.Load() > 0 && time.Now().Before(until) {
		time.Sleep(2 * time.Millisecond)
	}
}

// AlarmsDropped is the number of incidents a full alarm queue dropped.
func AlarmsDropped() int64 { return lost.Load() }

func crash(why string) {
	crashing.Store(true)
	fmt.Fprintf(os.Stderr, "serlog: %s; exiting\n", why)
	flushSlots(time.Second)
	os.Exit(70)
}
