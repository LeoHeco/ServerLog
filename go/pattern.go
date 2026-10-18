package serlog

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strconv"
	"strings"
	"unicode/utf8"
)

var (
	plainTokens = []string{"module", "func", "line", "thread", "tid", "task", "pid", "host", "app", "env",
		"instance", "tags", "elapsed", "seq", "spans"}
	tokenWords = map[string]string{"level": "lower", "file": "base", "msg": "q", "fields": "q", "error": "stack"}
	callerToks = []string{"module", "func", "file", "line"}
	token      = regexp.MustCompile(`\{\{|\}\}|\{([^}]*)\}|\}`)
	fit        = regexp.MustCompile(`^(?:([<>^])(\d+))?(?:\.(\d+))?$`)
	tilde      = regexp.MustCompile(`^~(\d+)`)
	pid        = strconv.Itoa(os.Getpid())
)

type piece struct {
	kind  string
	text  string
	on    bool
	short int
	align byte
	width int
	cut   int
}

// Pattern is a line template: `{time} {level:<5} {target}: {msg}{fields}`. Holes are `{token}` or
// `{token:spec}`; `{{` and `}}` are literal braces. Every token but `time` may end with `<N`, `>N` or `^N` to pad,
// then `.M` to cut. A preset name such as `tracing` stands for that logger's pattern and shows levels its way.
type Pattern struct {
	source     string
	pieces     []piece
	showsError bool
	caller     bool
	labels     Compat
}

// ParsePattern reads a pattern, or a preset name.
func ParsePattern(source string) (*Pattern, error) {
	p := &Pattern{source: source}
	s := source
	if c, err := ParseCompat(source); err == nil {
		s, p.labels = c.Pattern(), c
	}
	bad := func(why string) error { return fmt.Errorf("pattern `%s`: %s", s, why) }
	var lit strings.Builder
	at := 0
	for _, m := range token.FindAllStringSubmatchIndex(s, -1) {
		lit.WriteString(s[at:m[0]])
		at = m[1]
		switch g := s[m[0]:m[1]]; g {
		case "{{", "}}":
			lit.WriteByte(g[0])
			continue
		case "}":
			return nil, bad("stray `}`")
		}
		if lit.Len() > 0 {
			p.pieces = append(p.pieces, piece{text: lit.String()})
			lit.Reset()
		}
		name, spec, _ := strings.Cut(s[m[2]:m[3]], ":")
		h, err := hole(strings.TrimSpace(name), spec)
		if err != nil {
			return nil, bad(err.Error())
		}
		p.pieces = append(p.pieces, h)
		p.showsError = p.showsError || h.kind == "error"
		p.caller = p.caller || slices.Contains(callerToks, h.kind)
	}
	if strings.Contains(s[at:], "{") {
		return nil, bad("unclosed `{`")
	}
	lit.WriteString(s[at:])
	if lit.Len() > 0 {
		p.pieces = append(p.pieces, piece{text: lit.String()})
	}
	return p, nil
}

// MustPattern is ParsePattern for patterns known to be good.
func MustPattern(source string) *Pattern {
	p, err := ParsePattern(source)
	if err != nil {
		panic(err)
	}
	return p
}

func hole(name, spec string) (piece, error) {
	h := piece{kind: name, cut: -1}
	if name == "time" {
		h.text = spec
		return h, nil
	}
	rest := spec
	if word, ok := tokenWords[name]; ok {
		rest, h.on = strings.CutPrefix(spec, word)
	} else if name == "target" && strings.HasPrefix(spec, "~") {
		m := tilde.FindStringSubmatch(spec)
		if m == nil {
			return h, fmt.Errorf("bad `~` in `%s`", spec)
		}
		h.short, _ = strconv.Atoi(m[1])
		rest = spec[len(m[0]):]
	} else if name != "target" && !slices.Contains(plainTokens, name) {
		return h, fmt.Errorf("unknown token `%s`", name)
	}
	m := fit.FindStringSubmatch(rest)
	if m == nil {
		return h, fmt.Errorf("bad spec `%s` for `%s`", spec, name)
	}
	if m[1] != "" {
		h.align = m[1][0]
		h.width, _ = strconv.Atoi(m[2])
	}
	if m[3] != "" {
		h.cut, _ = strconv.Atoi(m[3])
	}
	return h, nil
}

func shorten(target string, n int) string {
	if len(target) <= n {
		return target
	}
	sep := "."
	if strings.Contains(target, "::") {
		sep = "::"
	}
	parts := strings.Split(target, sep)
	out := slices.Clone(parts)
	for i := 0; i < len(parts)-1 && len(strings.Join(out, sep)) > n; i++ {
		if parts[i] != "" {
			out[i] = parts[i][:1]
		}
	}
	return strings.Join(out, sep)
}

func align(s string, how byte, width int) string {
	padding := max(width-utf8.RuneCountInString(s), 0)
	switch how {
	case '<':
		return s + strings.Repeat(" ", padding)
	case '>':
		return strings.Repeat(" ", padding) + s
	case '^':
		return strings.Repeat(" ", padding/2) + s + strings.Repeat(" ", padding-padding/2)
	}
	return s
}

// Render lays the record out; a theme paints the level.
func (p *Pattern) Render(r *Record, t Theme) string {
	var b strings.Builder
	for _, h := range p.pieces {
		if h.kind == "" {
			b.WriteString(h.text)
			continue
		}
		s := p.token(h, r)
		if h.cut >= 0 && utf8.RuneCountInString(s) > h.cut {
			s = string([]rune(s)[:h.cut])
		}
		s = align(s, h.align, h.width)
		if h.kind == "level" && t != nil {
			s = t.of(r.Level).paint(true, s)
		}
		b.WriteString(s)
	}
	return b.String()
}

func (p *Pattern) token(h piece, r *Record) string {
	switch h.kind {
	case "time":
		return writeTime(h.text, r.Time)
	case "level":
		if h.on {
			return r.Level.String()
		}
		c := p.labels
		if c == "" {
			c = compat()
		}
		return r.Level.Label(c)
	case "target":
		if h.short > 0 {
			return shorten(r.Target, h.short)
		}
		return r.Target
	case "module":
		return or(r.Module, r.Target)
	case "func":
		return or(r.Func, "?")
	case "file":
		if r.File == "" {
			return "?"
		}
		if h.on {
			return filepath.Base(r.File)
		}
		return r.File
	case "line":
		if r.Line > 0 {
			return strconv.Itoa(r.Line)
		}
		return "?"
	case "thread":
		return or(r.Thread, "?")
	case "tid", "task":
		return "?"
	case "pid":
		return pid
	case "host":
		return hostName()
	case "app":
		return meta().App
	case "env":
		return meta().Env
	case "instance":
		return meta().Instance
	case "msg":
		if h.on {
			return logfmtValue(r.Msg)
		}
		return r.Msg
	case "fields":
		if p.showsError {
			return fieldText(r, h.on, "error")
		}
		return fieldText(r, h.on)
	case "tags":
		return strings.Join(r.Tags, ",")
	case "error":
		if e, ok := r.Field("error"); ok {
			return show(e)
		}
		return ""
	case "elapsed":
		return strconv.FormatInt(elapsed(), 10)
	case "seq":
		return strconv.FormatUint(r.Seq, 10)
	case "spans":
		if r.Spans != "" {
			return r.Spans + ": "
		}
		return ""
	}
	return ""
}

// Caller is whether the pattern shows where a record was made.
func (p *Pattern) Caller() bool { return p.caller }

func (p *Pattern) String() string { return p.source }

func or(s, fallback string) string {
	if s == "" {
		return fallback
	}
	return s
}
