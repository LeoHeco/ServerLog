package serlog

import (
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

// clean sets the level to trace and removes the slots a test adds.
func clean(t *testing.T) {
	t.Helper()
	SetLevel(LevelTrace)
	if !slices.ContainsFunc(Statuses(), func(s Status) bool { return s.Name == "main" }) {
		if _, err := Add(&Memory{Slot: Slot{Name: "main", Filter: "off"}, Size: 1}); err != nil {
			t.Fatal(err)
		}
	}
	var before []string
	for _, s := range Statuses() {
		before = append(before, s.Name)
	}
	t.Cleanup(func() {
		for _, s := range Statuses() {
			if !slices.Contains(before, s.Name) {
				Remove(s.Name)
			}
		}
	})
}

func status(t *testing.T, name string) Status {
	t.Helper()
	for _, s := range Statuses() {
		if s.Name == name {
			return s
		}
	}
	t.Fatalf("no slot %q", name)
	return Status{}
}

func must[T any](t *testing.T) func(T, error) T {
	return func(v T, err error) T {
		t.Helper()
		if err != nil {
			t.Fatal(err)
		}
		return v
	}
}

type flaky struct {
	Slot
	fail atomic.Bool
	seen atomic.Int64
}

func (f *flaky) Kind() string { return "flaky" }

func (f *flaky) Emit(*Entry) error {
	f.seen.Add(1)
	if f.fail.Load() {
		return errors.New("boom")
	}
	return nil
}

func only(target string) string { return target + "=trace" }

func TestAFailureIsToldOnceAndItsRecoveryAfter(t *testing.T) {
	clean(t)
	alarms := &Memory{Slot: Slot{Name: "alarms1", AlarmOnly: true}, Size: 10}
	must[string](t)(Add(alarms))
	f := &flaky{Slot: Slot{Name: "flaky1", Filter: only("t1"), Probe: time.Nanosecond, Alarm: &Tiers{Tiers: [][]string{{"alarms1"}}}}}
	f.fail.Store(true)
	must[string](t)(Add(f))
	log := Get("t1")
	for range 3 {
		log.Info("write")
	}
	Flush()
	if f.seen.Load() != 3 || status(t, "flaky1").Health != Down {
		t.Fatalf("seen %d, health %s", f.seen.Load(), status(t, "flaky1").Health)
	}
	told := alarms.Records()
	if len(told) != 1 || told[0].Level != LevelError || told[0].Msg != "sink `flaky1` failed: boom" {
		t.Fatalf("told %+v", told)
	}
	if p, _ := told[0].Field("phase"); p != "opened" {
		t.Fatalf("phase %v", p)
	}
	f.fail.Store(false)
	log.Info("write")
	Flush()
	if h := status(t, "flaky1").Health; h != Up {
		t.Fatalf("health %s", h)
	}
	var phases []any
	for _, r := range alarms.Records() {
		p, _ := r.Field("phase")
		phases = append(phases, p)
	}
	if !slices.Equal(phases, []any{"opened", "resolved"}) {
		t.Fatalf("phases %v", phases)
	}
}

func TestADownSinkIsSkippedUntilItsProbe(t *testing.T) {
	clean(t)
	f := &flaky{}
	f.fail.Store(true)
	must[string](t)(Add(With(f, Slot{Name: "flaky2", Filter: only("t2"), Probe: time.Hour, OnFail: "ignore"})))
	for range 3 {
		Get("t2").Info("write")
	}
	s := status(t, "flaky2")
	if f.seen.Load() != 1 || s.Health != Down || s.Skipped != 2 || s.Error != "boom" {
		t.Fatalf("seen %d, %+v", f.seen.Load(), s)
	}
}

func TestThePanicPolicyPanicsInTheCaller(t *testing.T) {
	clean(t)
	f := &flaky{Slot: Slot{Name: "flaky3", Filter: only("t3"), OnFail: "panic"}}
	f.fail.Store(true)
	must[string](t)(Add(f))
	for range 2 {
		func() {
			defer func() {
				var e *SinkError
				if p := recover(); p == nil || !errors.As(p.(error), &e) || e.Sink != "flaky3" {
					t.Fatalf("recovered %v", p)
				}
			}()
			Get("t3").Error("write")
		}()
	}
	if f.seen.Load() != 2 {
		t.Fatalf("seen %d", f.seen.Load())
	}
}

func TestNamesClaimsAndChecksAreEnforced(t *testing.T) {
	clean(t)
	a := must[string](t)(Add(&Memory{Slot: Slot{Filter: "off"}}))
	b := must[string](t)(Add(&Memory{Slot: Slot{Filter: "off"}}))
	if !strings.HasPrefix(a, "memory-") || !strings.HasPrefix(b, "memory-") || a == b {
		t.Fatal(a, b)
	}
	var taken *NameTakenError
	if _, err := Add(&Memory{Slot: Slot{Name: "main"}}); !errors.As(err, &taken) {
		t.Fatal(err)
	}
	if _, err := Add(&Memory{Slot: Slot{Name: "*"}}); !errors.As(err, &taken) {
		t.Fatal(err)
	}
	dir := t.TempDir()
	path := filepath.Join(dir, "one.log")
	must[string](t)(Add(&File{Slot: Slot{Name: "one", Filter: "off"}, Path: path}))
	var claimed *ClaimedError
	if _, err := Add(&File{Slot: Slot{Name: "two"}, Path: path}); !errors.As(err, &claimed) {
		t.Fatal(err)
	}
	var ready *NotReadyError
	if _, err := Add(&File{Slot: Slot{Name: "three"}, Path: dir}); !errors.As(err, &ready) {
		t.Fatal(err)
	}
	if !Remove("one") || Remove("one") {
		t.Fatal("remove")
	}
}

func TestAQueuedSlotWritesInTheBackground(t *testing.T) {
	clean(t)
	m := &Memory{Slot: Slot{Name: "queued", Filter: only("t5"), Queue: 100, Wait: 20 * time.Millisecond}, Size: 100}
	must[string](t)(Add(m))
	for n := range 5 {
		Get("t5").Info("queued", "n", n)
	}
	Flush()
	if m.Len() != 5 {
		t.Fatalf("kept %d", m.Len())
	}
}

func TestSlotsLayOutAndProcessOnTheirOwn(t *testing.T) {
	clean(t)
	js := &Memory{Slot: Slot{Name: "json", Filter: only("t6"), Format: "json"}}
	text := &Memory{Slot: Slot{Name: "text", Filter: only("t6"), Pattern: "{level}:{msg}", Process: []Processor{&Match{Tags: []string{"keep"}}}}}
	must[string](t)(Add(js))
	must[string](t)(Add(text))
	log := Get("t6")
	log.Info("kept", Tags("keep"))
	log.Info("dropped by text")
	if js.Len() != 2 || !strings.HasPrefix(js.Lines()[0], `{"time":`) {
		t.Fatal(js.Lines())
	}
	if !slices.Equal(text.Lines(), []string{"INFO:kept"}) {
		t.Fatal(text.Lines())
	}
}

func TestRecordsMadeInsideASinkAreSuppressed(t *testing.T) {
	clean(t)
	var seen atomic.Int64
	loud := SinkFunc(func(e *Entry) error {
		seen.Add(1)
		Get("t7").InfoContext(e.Context(), "from inside, with the context")
		Get("t7").Info("from inside, without it")
		return nil
	})
	must[string](t)(Add(With(loud, Slot{Name: "loud", Filter: only("t7")})))
	Get("t7").Info("outside")
	if seen.Load() != 1 || status(t, "loud").Suppressed != 2 {
		t.Fatalf("seen %d, suppressed %d", seen.Load(), status(t, "loud").Suppressed)
	}
}

func TestRecordsMadeInAQueuedSinkAreSuppressed(t *testing.T) {
	clean(t)
	var seen atomic.Int64
	slow := SinkFunc(func(*Entry) error {
		seen.Add(1)
		Get("t9").Info("from inside, later")
		return nil
	})
	must[string](t)(Add(With(slow, Slot{Name: "slow", Filter: only("t9"), Queue: 10, Wait: time.Millisecond})))
	Get("t9").Info("outside")
	Flush()
	if seen.Load() != 1 || status(t, "slow").Suppressed != 1 {
		t.Fatalf("seen %d, suppressed %d", seen.Load(), status(t, "slow").Suppressed)
	}
}

func TestFunctionsAreSinks(t *testing.T) {
	clean(t)
	var got []string
	must[string](t)(Add(With(SinkFunc(func(e *Entry) error {
		got = append(got, e.Line())
		return nil
	}), Slot{Name: "fn", Filter: only("t8"), Pattern: "{msg}"})))
	Get("t8").Warn("hi")
	if !slices.Equal(got, []string{"hi"}) {
		t.Fatal(got)
	}
}

func TestRemovingASlotWritesItsQueueAndClosesItsSink(t *testing.T) {
	clean(t)
	m := &Memory{Slot: Slot{Name: "leaving", Filter: only("t11"), Queue: 10, Wait: time.Hour}}
	must[string](t)(Add(m))
	for range 3 {
		Get("t11").Info("queued")
	}
	path := filepath.Join(t.TempDir(), "gone.log")
	must[string](t)(Add(&File{Slot: Slot{Name: "gone", Filter: "off"}, Path: path}))
	if !Remove("leaving") || !Remove("gone") || m.Len() != 3 {
		t.Fatalf("kept %d", m.Len())
	}
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
}
