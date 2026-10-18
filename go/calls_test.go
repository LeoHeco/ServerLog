package serlog

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"slices"
	"strings"
	"testing"
	"testing/slogtest"
	"time"
)

func calls(t *testing.T) *Memory {
	t.Helper()
	clean(t)
	m := &Memory{Slot: Slot{Filter: "calls=trace,slogtest=trace," + root() + "=trace", Pattern: "{file}:{func} {msg}"}, Size: 100}
	must[string](t)(Add(m))
	return m
}

func last(t *testing.T, m *Memory, msg string) *Record {
	t.Helper()
	r := m.Last(msg)
	if r == nil {
		t.Fatalf("no record %q in %v", msg, m.Lines())
	}
	return r
}

func TestMessagesFieldsAndTheCaller(t *testing.T) {
	m := calls(t)
	sync := Get("calls", "mirror", "debian")
	sync.Info("synced 42 files", "tries", 3, "ratio", 0.5, "ok", true, Tags("sync"))
	r := last(t, m, "synced 42 files")
	want := []Field{{"mirror", "debian"}, {"tries", int64(3)}, {"ratio", 0.5}, {"ok", true}}
	if fmt.Sprint(r.Fields) != fmt.Sprint(want) || !slices.Equal(r.Tags, []string{"sync"}) {
		t.Fatalf("%+v %v", r.Fields, r.Tags)
	}
	if r.Func != "TestMessagesFieldsAndTheCaller" || !strings.HasSuffix(r.File, "calls_test.go") || r.Line == 0 {
		t.Fatalf("caller %s %s:%d", r.Func, r.File, r.Line)
	}
	if r.Module != "github.com/LeoHeco/ServerLog/go" {
		t.Fatalf("module %s", r.Module)
	}
}

func TestPackageCallsUseTheAppsName(t *testing.T) {
	m := calls(t)
	Info("plain call")
	r := last(t, m, "plain call")
	if r.Target != root() || r.Func != "TestPackageCallsUseTheAppsName" {
		t.Fatalf("%s %s", r.Target, r.Func)
	}
}

func TestLevelsAndSlogLevels(t *testing.T) {
	m := calls(t)
	log := Get("calls")
	ctx := context.Background()
	log.Log(ctx, LevelTrace.Slog(), "t")
	log.Warn("w")
	log.Log(ctx, LevelFatal.Slog(), "f")
	log.Log(ctx, slog.LevelError+2, "e")
	var got []Level
	for _, msg := range []string{"t", "w", "f", "e"} {
		got = append(got, last(t, m, msg).Level)
	}
	if !slices.Equal(got, []Level{LevelTrace, LevelWarn, LevelFatal, LevelError}) {
		t.Fatal(got)
	}
}

type costly struct{ hit *int }

func (c costly) LogValue() slog.Value {
	*c.hit++
	return slog.StringValue("worked")
}

func TestValuesAreNotWorkedOutBelowTheLevel(t *testing.T) {
	m := calls(t)
	SetLevel(LevelInfo)
	hit := 0
	Get("calls").Debug("hidden", "dump", costly{&hit})
	if hit != 0 {
		t.Fatal(hit)
	}
	Get("calls").Info("shown", "dump", costly{&hit})
	if v, _ := last(t, m, "shown").Field("dump"); v != "worked" || hit != 1 {
		t.Fatal(v, hit)
	}
}

func TestBindSpansTheContext(t *testing.T) {
	m := calls(t)
	ctx := Bind(context.Background(), "request", 7)
	log := Get("calls")
	log.InfoContext(ctx, "inside", "extra", 1)
	log.InfoContext(Bind(ctx, "step", 2), "nested")
	log.Info("outside")
	if got := fmt.Sprint(last(t, m, "inside").Fields); got != "[{request 7} {extra 1}]" {
		t.Fatal(got)
	}
	if got := fmt.Sprint(last(t, m, "nested").Fields); got != "[{request 7} {step 2}]" {
		t.Fatal(got)
	}
	if len(last(t, m, "outside").Fields) != 0 {
		t.Fatal("outside has fields")
	}
}

func TestGroupsNest(t *testing.T) {
	m := calls(t)
	Get("calls").WithGroup("req").Info("grouped", "id", 9, slog.Group("user", "name", "ann"))
	r := last(t, m, "grouped")
	if got := fmt.Sprint(r.Fields); got != "[{req [{id 9} {user [{name ann}]}]}]" {
		t.Fatal(got)
	}
	if got := fieldText(r, false); got != " req.id=9 req.user.name=ann" {
		t.Fatal(got)
	}
}

func TestErrorsCarryTheirCauses(t *testing.T) {
	m := calls(t)
	err := fmt.Errorf("parse failed: %w", errors.New("bad input"))
	Get("calls").Error("gave up", Err(err))
	Get("calls").Error("gave up again", "err", err)
	for _, msg := range []string{"gave up", "gave up again"} {
		v, ok := last(t, m, msg).Field("error")
		if !ok || show(v) != "parse failed: bad input" || jsonValue(v) != `{"message":"parse failed","causes":["bad input"]}` {
			t.Fatal(msg, v)
		}
	}
}

func TestCaptureRoutesSlogAndLog(t *testing.T) {
	m := calls(t)
	was := slog.Default()
	t.Cleanup(func() { slog.SetDefault(was) })
	Capture()
	slog.Info("from slog", "n", 1)
	if r := last(t, m, "from slog"); r.Target != root() {
		t.Fatal(r.Target)
	}
}

func TestSlogtest(t *testing.T) {
	m := calls(t)
	h := Handler("slogtest")
	results := func() []map[string]any {
		var out []map[string]any
		for _, r := range m.Records() {
			if r.Target != "slogtest" {
				continue
			}
			e := map[string]any{slog.LevelKey: r.Level.Slog(), slog.MessageKey: r.Msg}
			if !r.Time.IsZero() {
				e[slog.TimeKey] = r.Time
			}
			var put func(dst map[string]any, fs []Field)
			put = func(dst map[string]any, fs []Field) {
				for _, f := range fs {
					if g, ok := f.Value.(Group); ok {
						sub := map[string]any{}
						put(sub, g)
						dst[f.Key] = sub
						continue
					}
					switch v := f.Value.(type) {
					case time.Time, time.Duration:
						dst[f.Key] = v
					default:
						dst[f.Key] = v
					}
				}
			}
			put(e, r.Fields)
			out = append(out, e)
		}
		return out
	}
	if err := slogtest.TestHandler(h, results); err != nil {
		t.Fatal(err)
	}
}
