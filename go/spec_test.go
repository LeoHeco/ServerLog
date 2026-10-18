package serlog

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"
)

func vectors(t *testing.T, name string) map[string]any {
	t.Helper()
	b, err := os.ReadFile(filepath.Join("..", "spec", "cases", name+".json"))
	if err != nil {
		t.Fatal(err)
	}
	var out map[string]any
	d := json.NewDecoder(strings.NewReader(string(b)))
	d.UseNumber()
	if err := d.Decode(&out); err != nil {
		t.Fatal(err)
	}
	return normalise(out).(map[string]any)
}

func withMeta(t *testing.T, m Meta) {
	was := meta()
	change(func(s *state) { s.meta = m })
	t.Cleanup(func() { change(func(s *state) { s.meta = was }) })
}

func vectorValue(v any) any {
	switch x := v.(type) {
	case map[string]any:
		if msgs, ok := x["$error"].([]any); ok {
			var err error
			for i := len(msgs) - 1; i >= 0; i-- {
				if err == nil {
					err = errors.New(msgs[i].(string))
				} else {
					err = fmt.Errorf("%s: %w", msgs[i], err)
				}
			}
			return err
		}
		keys := slices.Sorted(func(yield func(string) bool) {
			for k := range x {
				if !yield(k) {
					return
				}
			}
		})
		g := Group{}
		for _, k := range keys {
			g = append(g, Field{k, vectorValue(x[k])})
		}
		return g
	case []any:
		out := make([]any, len(x))
		for i, e := range x {
			out[i] = vectorValue(e)
		}
		return out
	}
	return v
}

func vectorRecord(t *testing.T, r map[string]any) *Record {
	l, err := ParseLevel(r["level"].(string))
	if err != nil {
		t.Fatal(err)
	}
	rec := &Record{Level: l, Target: r["target"].(string), Msg: r["msg"].(string), Time: time.Unix(r["time"].(int64), 0)}
	for _, f := range r["fields"].([]any) {
		pair := f.([]any)
		rec.Fields = append(rec.Fields, Field{pair[0].(string), vectorValue(pair[1])})
	}
	if f, ok := r["file"].(string); ok {
		rec.File = f
	}
	if n, ok := r["line"].(int64); ok {
		rec.Line = int(n)
	}
	return rec
}

func TestRenders(t *testing.T) {
	v := vectors(t, "render")
	m := v["meta"].(map[string]any)
	withMeta(t, Meta{m["app"].(string), m["env"].(string), m["instance"].(string)})
	for _, raw := range v["cases"].([]any) {
		c := raw.(map[string]any)
		r := vectorRecord(t, c["record"].(map[string]any))
		t.Run(r.Msg, func(t *testing.T) {
			for _, p := range c["patterns"].([]any) {
				p := p.(map[string]any)
				if got := MustPattern(p["pattern"].(string)).Render(r, nil); got != p["expect"] {
					t.Errorf("%s: got %q, want %q", p["pattern"], got, p["expect"])
				}
			}
			for _, p := range c["presets"].([]any) {
				p := p.(map[string]any)
				got := MustPattern(p["name"].(string)).Render(r, nil)
				if want, ok := p["expect"].(string); ok && got != want {
					t.Errorf("%s: got %q, want %q", p["name"], got, want)
				}
				if tail, ok := p["tail"].(string); ok && !strings.HasSuffix(got, tail) {
					t.Errorf("%s: %q does not end with %q", p["name"], got, tail)
				}
			}
			if want, ok := c["logfmt"].(string); ok {
				if got := Logfmt.Render(r, nil); got != want {
					t.Errorf("logfmt: got %q, want %q", got, want)
				}
			}
			if want, ok := c["pretty"].(string); ok {
				if got := Pretty.Render(r, nil); !strings.HasSuffix(got, want) {
					t.Errorf("pretty: %q does not end with %q", got, want)
				}
			}
			if got := (&JSON{}).Render(r, nil); got != c["json"] {
				t.Errorf("json: got %s, want %s", got, c["json"])
			}
			for _, s := range c["schemas"].([]any) {
				s := s.(map[string]any)
				line := (&JSON{Schema: s["schema"].(string)}).Render(r, nil)
				var keys []string
				d := json.NewDecoder(strings.NewReader(line))
				d.Token()
				for d.More() {
					k, _ := d.Token()
					keys = append(keys, k.(string))
					var skip any
					d.Decode(&skip)
				}
				want := []string{}
				for _, k := range s["keys"].([]any) {
					want = append(want, k.(string))
				}
				if !slices.Equal(keys, want) {
					t.Errorf("%s keys: got %v, want %v", s["schema"], keys, want)
				}
				var got map[string]any
				json.Unmarshal([]byte(line), &got)
				for k, w := range s["has"].(map[string]any) {
					a, _ := json.Marshal(normalise(got[k]))
					b, _ := json.Marshal(w)
					if string(a) != string(b) {
						t.Errorf("%s.%s: got %s, want %s", s["schema"], k, a, b)
					}
				}
			}
		})
	}
}

func TestFilters(t *testing.T) {
	for _, raw := range vectors(t, "render")["filters"].([]any) {
		c := raw.(map[string]any)
		spec := c["spec"].(string)
		t.Run(spec, func(t *testing.T) {
			f := MustFilter(spec)
			msgs, _ := c["messages"].(map[string]any)
			for key, want := range map[string]bool{"allow": true, "deny": false} {
				for _, p := range c[key].([]any) {
					pair := p.([]any)
					l, err := ParseLevel(pair[0].(string))
					if err != nil {
						t.Fatal(err)
					}
					if got := f.Allows(l, pair[1].(string)); got != want {
						t.Errorf("%s %s: got %v", pair[0], pair[1], got)
					}
				}
				if m, ok := msgs[key].([]any); ok {
					for _, msg := range m {
						if got := f.Admits(msg.(string)); got != want {
							t.Errorf("%q: got %v", msg, got)
						}
					}
				}
			}
		})
	}
}

func num(v any) float64 {
	f, _ := toFloat(v)
	return f
}

func TestHealth(t *testing.T) {
	for _, raw := range vectors(t, "fail")["cases"].([]any) {
		c := raw.(map[string]any)
		t.Run(c["name"].(string), func(t *testing.T) {
			b := newBreaker(num(c["probe"]), 0)
			how := map[string]tell{"report": report, "silent": silent, "folded": folded}[c["tell"].(string)]
			for i, raw := range c["steps"].([]any) {
				s := raw.(map[string]any)
				at := num(s["at"])
				tried := b.admit(at)
				if tried != s["tried"] {
					t.Fatalf("#%d tried %v", i, tried)
				}
				var p Phase
				if tried {
					if s["result"] == "ok" {
						p = b.passed()
					} else {
						p = b.failed(at, "boom", num(c["remind"]), how)
					}
				}
				h, _, _, _, _ := b.snapshot()
				if string(h) != s["health"] {
					t.Errorf("#%d health %s, want %v", i, h, s["health"])
				}
				want, _ := s["phase"].(string)
				if string(p) != want {
					t.Errorf("#%d phase %q, want %q", i, p, want)
				}
			}
			if _, _, skipped, _, _ := b.snapshot(); skipped != c["skipped"].(int64) {
				t.Errorf("skipped %d, want %v", skipped, c["skipped"])
			}
		})
	}
}

type script struct {
	c     map[string]any
	mu    sync.Mutex
	tried []string
}

func strs(v any) []string {
	var out []string
	for _, x := range v.([]any) {
		out = append(out, x.(string))
	}
	return out
}

func (s *script) logs() []string { return strs(s.c["logs"]) }

func (s *script) resting(name string) bool { return slices.Contains(strs(s.c["down"]), name) }

func (s *script) deliver(name string, _ *Record) error {
	s.mu.Lock()
	s.tried = append(s.tried, name)
	s.mu.Unlock()
	if s.c["results"].(map[string]any)[name] != "ok" {
		return errors.New("refused")
	}
	return nil
}

func TestAlarmWalk(t *testing.T) {
	for _, raw := range vectors(t, "alarm")["cases"].([]any) {
		c := raw.(map[string]any)
		t.Run(c["name"].(string), func(t *testing.T) {
			var tiers [][]string
			for _, tier := range c["tiers"].([]any) {
				tiers = append(tiers, strs(tier))
			}
			d := &script{c: c}
			i := NewIncident(c["failing"].(string), "custom", Opened, "boom")
			heard := sound(&Tiers{Tiers: tiers}, i, d)
			if want := c["outcome"] == "heard"; heard != want {
				t.Errorf("heard %v", heard)
			}
			left := slices.Clone(d.tried)
			var got [][]string
			for _, tier := range tiers {
				var members []string
				for _, n := range tier {
					if n == "*" {
						members = append(members, d.logs()...)
					} else {
						members = append(members, n)
					}
				}
				var here []string
				left = slices.DeleteFunc(left, func(n string) bool {
					if slices.Contains(members, n) {
						here = append(here, n)
						return true
					}
					return false
				})
				if len(here) > 0 {
					slices.Sort(here)
					got = append(got, here)
				}
			}
			var want [][]string
			for _, tier := range c["tried"].([]any) {
				w := strs(tier)
				slices.Sort(w)
				want = append(want, w)
			}
			if fmt.Sprint(got) != fmt.Sprint(want) {
				t.Errorf("tried %v, want %v", got, want)
			}
			var fails []string
			for _, f := range i.Failures {
				fails = append(fails, f.Sink)
			}
			if !slices.Equal(fails, strs(c["failures"])) {
				t.Errorf("failures %v, want %v", fails, c["failures"])
			}
		})
	}
}
