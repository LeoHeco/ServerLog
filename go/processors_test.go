package serlog

import (
	"fmt"
	"testing"
	"time"
)

func rec(msg string, fields ...Field) *Record { return NewRecord(LevelInfo, "t", msg, fields...) }

func TestRedactsKeysPathsAndPatterns(t *testing.T) {
	r := rec("call 13800138000 now",
		Field{"pass", "hunter2"},
		Field{"req", Group{{"headers", Group{{"authorization", "Bearer x"}, {"accept", "*/*"}}}}},
		Field{"user", "ann"})
	out := (&Redact{Keys: []string{"pass"}, Paths: []string{"req.headers.authorization"}, Patterns: []string{`\d{11}`}}).Process(r)
	if v, _ := out.Field("pass"); v != "***" {
		t.Fatal(v)
	}
	if v, _ := out.Field("user"); v != "ann" {
		t.Fatal(v)
	}
	if v, _ := out.Field("req"); fmt.Sprint(v) != "[{headers [{authorization ***} {accept */*}]}]" {
		t.Fatal(v)
	}
	if out.Msg != "call *** now" {
		t.Fatal(out.Msg)
	}
	if v, _ := r.Field("pass"); v != "hunter2" {
		t.Fatal("the original changed")
	}
}

func TestLeavesCleanRecordsAlone(t *testing.T) {
	r := rec("fine", Field{"n", 1})
	if (&Redact{Keys: []string{"pass"}}).Process(r) != r || (Sanitise{}).Process(r) != r {
		t.Fatal("copied a clean record")
	}
}

func TestSanitisesLineBreaks(t *testing.T) {
	out := Sanitise{}.Process(rec("x\r\ny\x07", Field{"who", "a\nb"}))
	if out.Msg != `x\r\ny\u{0007}` {
		t.Fatal(out.Msg)
	}
	if v, _ := out.Field("who"); v != `a\nb` {
		t.Fatal(v)
	}
}

func TestAddsMatchesAndExcludes(t *testing.T) {
	if v, _ := (Fields{"region": "cn"}).Process(rec("x")).Field("region"); v != "cn" {
		t.Fatal(v)
	}
	r := rec("x", Field{"mirror", "debian"})
	r.Level, r.Tags = LevelWarn, []string{"audit"}
	if (&Match{Tags: []string{"audit"}, Fields: map[string]any{"mirror": "debian"}, Level: LevelWarn}).Process(r) != r {
		t.Fatal("match")
	}
	if (&Match{Tags: []string{"other"}}).Process(r) != nil {
		t.Fatal("other tag")
	}
	if (&Match{Tags: []string{"audit"}, Exclude: true}).Process(r) != nil {
		t.Fatal("exclude")
	}
	if (&Match{Level: LevelError, Exclude: true}).Process(r) != r {
		t.Fatal("exclude by level")
	}
}

func TestSamplesLimitsAndDedupes(t *testing.T) {
	s := &Sample{First: 2, Every: 3, Per: time.Minute}
	var kept []bool
	for range 8 {
		kept = append(kept, s.Process(rec("same")) != nil)
	}
	if fmt.Sprint(kept) != "[true true false false true false false true]" {
		t.Fatal(kept)
	}
	l := &Limit{Rate: 0, Burst: 3}
	n := 0
	for range 5 {
		if l.Process(rec("x")) != nil {
			n++
		}
	}
	if n != 3 {
		t.Fatal(n)
	}
	d := &Dedupe{Within: 200 * time.Millisecond}
	if d.Process(rec("again")) == nil || d.Process(rec("again")) != nil || d.Process(rec("again")) != nil {
		t.Fatal("dedupe")
	}
	time.Sleep(250 * time.Millisecond)
	if v, _ := d.Process(rec("again")).Field("repeated"); v != 2 {
		t.Fatal(v)
	}
}
