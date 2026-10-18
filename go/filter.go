package serlog

import (
	"cmp"
	"regexp"
	"slices"
	"strconv"
	"strings"
)

// Filter decides which records pass, by target and level, written the RUST_LOG way:
// `info,app=debug,app.db=trace,hyper=off,web:*=warn/regex`. The most specific directive wins; anything after the
// first `/` is a regex the message must match. Targets no directive matches are off unless a default level is
// given, and an empty spec lets `error` and above through.
type Filter struct {
	spec    string
	base    *Level
	rules   []rule
	pattern *regexp.Regexp
}

type rule struct {
	target string
	glob   *regexp.Regexp
	level  *Level
}

func (r rule) rank() int {
	n := -len(strings.ReplaceAll(r.target, "*", ""))
	if r.glob != nil {
		n += 1_000_000
	}
	return n
}

func (r rule) covers(t string) bool {
	if r.glob != nil {
		return r.glob.MatchString(t)
	}
	rest, ok := strings.CutPrefix(t, r.target)
	if !ok {
		return false
	}
	return rest == "" || slices.ContainsFunc([]string{"::", ".", ":", "/"}, func(s string) bool {
		return strings.HasPrefix(rest, s)
	})
}

func offOr(s string) (*Level, error) {
	if strings.EqualFold(s, "off") {
		return nil, nil
	}
	l, err := ParseLevel(s)
	return &l, err
}

// ParseFilter reads comma-separated directives, optionally followed by `/regex`.
func ParseFilter(spec string) (*Filter, error) {
	f := &Filter{spec: spec}
	directives, regex, hasRegex := strings.Cut(spec, "/")
	if strings.TrimSpace(directives) == "" {
		e := LevelError
		f.base = &e
	}
	for d := range strings.SplitSeq(directives, ",") {
		d = strings.TrimSpace(d)
		if d == "" {
			continue
		}
		if target, level, ok := strings.Cut(d, "="); ok {
			l, err := offOr(strings.TrimSpace(level))
			if err != nil {
				return nil, err
			}
			f.rules = append(f.rules, newRule(strings.TrimSpace(target), l))
			continue
		}
		if l, err := offOr(d); err == nil {
			f.base = l
		} else {
			zero := Level(0)
			f.rules = append(f.rules, newRule(d, &zero))
		}
	}
	slices.SortStableFunc(f.rules, func(a, b rule) int { return cmp.Compare(a.rank(), b.rank()) })
	if hasRegex {
		re, err := regexp.Compile(regex)
		if err != nil {
			return nil, err
		}
		f.pattern = re
	}
	return f, nil
}

// MustFilter is ParseFilter for specs known to be good.
func MustFilter(spec string) *Filter {
	f, err := ParseFilter(spec)
	if err != nil {
		panic(err)
	}
	return f
}

// LevelFilter lets `level` and above through for every target.
func LevelFilter(l Level) *Filter {
	name := l.String()
	if name == "custom" {
		name = strconv.Itoa(int(l))
	}
	return MustFilter(name)
}

func newRule(target string, l *Level) rule {
	r := rule{target: target, level: l}
	if strings.Contains(target, "*") {
		parts := strings.Split(target, "*")
		for i, p := range parts {
			parts[i] = regexp.QuoteMeta(p)
		}
		r.glob = regexp.MustCompile("^" + strings.Join(parts, ".*") + "$")
	}
	return r
}

// Allows is whether a record at `level` from `target` passes, before its message is looked at.
func (f *Filter) Allows(level Level, target string) bool {
	min := f.base
	for _, r := range f.rules {
		if r.covers(target) {
			min = r.level
			break
		}
	}
	return min != nil && level >= *min
}

// Admits is whether the message passes the `/regex` part, if there is one.
func (f *Filter) Admits(msg string) bool { return f.pattern == nil || f.pattern.MatchString(msg) }

// Floor is the lowest level any target lets through; false when everything is off.
func (f *Filter) Floor() (Level, bool) {
	var out *Level
	consider := func(l *Level) {
		if l != nil && (out == nil || *l < *out) {
			out = l
		}
	}
	consider(f.base)
	for _, r := range f.rules {
		consider(r.level)
	}
	if out == nil {
		return 0, false
	}
	return *out, true
}

func (f *Filter) String() string { return f.spec }
