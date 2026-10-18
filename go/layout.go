package serlog

import (
	"fmt"
	"strings"
	"sync"
)

// Layout turns a record into one line; the theme is nil when the line is not coloured.
type Layout interface {
	Render(r *Record, t Theme) string
}

// LayoutFunc is a function used as a Layout.
type LayoutFunc func(r *Record) string

// Render calls f.
func (f LayoutFunc) Render(r *Record, _ Theme) string { return f(r) }

type callerShower interface{ Caller() bool }

// showsCaller is whether lines laid out by l may show the caller, which costs a frame lookup per record.
func showsCaller(l Layout) bool {
	if c, ok := l.(callerShower); ok {
		return c.Caller()
	}
	return true
}

type inherit struct{}

var profiles sync.Map

func profile(c Compat) *Pattern {
	if p, ok := profiles.Load(c); ok {
		return p.(*Pattern)
	}
	p, _ := profiles.LoadOrStore(c, MustPattern(string(c)))
	return p.(*Pattern)
}

func (inherit) Render(r *Record, t Theme) string { return profile(compat()).Render(r, t) }
func (inherit) Caller() bool                     { return profile(compat()).Caller() }

type logfmt struct{}

func (logfmt) Render(r *Record, _ Theme) string { return logfmtLine(r) }
func (logfmt) Caller() bool                     { return false }

type pretty struct{}

func (pretty) Render(r *Record, t Theme) string { return prettyLine(r, t) }
func (pretty) Caller() bool                     { return false }

// The built-in layouts besides patterns and JSON.
var (
	// Inherit is the pattern of the compat profile in force.
	Inherit Layout = inherit{}
	// Logfmt writes `time=… level=… target=… msg=… key=value…`.
	Logfmt Layout = logfmt{}
	// Pretty writes a first line plus one aligned line per field, for reading in a terminal.
	Pretty Layout = pretty{}
)

// buildLayout is the layout a slot asks for, or nil for the sink's own.
func buildLayout(s *Slot) (Layout, error) {
	if s.Layout != nil {
		return s.Layout, nil
	}
	if s.LayoutName != "" {
		return layoutKind(s.LayoutName)
	}
	jsonish := s.Schema != "" || len(s.Keys) > 0 || s.Levels != "" || s.Nest != ""
	if s.Pattern != "" && s.Format != "" && s.Format != "text" {
		return nil, configErr("`pattern` goes with `format: text`, not `%s`", s.Format)
	}
	json := func() (Layout, error) {
		j := &JSON{Schema: s.Schema, Keys: s.Keys, Levels: s.Levels, Nest: s.Nest}
		if err := j.check(); err != nil {
			return nil, &ConfigError{Msg: err.Error()}
		}
		return j, nil
	}
	switch strings.ToLower(s.Format) {
	case "json":
		return json()
	case "logfmt":
		return Logfmt, nil
	case "pretty":
		return Pretty, nil
	case "", "text":
		if s.Pattern != "" {
			p, err := ParsePattern(s.Pattern)
			if err != nil {
				return nil, &ConfigError{Msg: err.Error(), Err: err}
			}
			return p, nil
		}
		if jsonish && s.Format == "" {
			return json()
		}
		return nil, nil
	}
	return nil, &ConfigError{Msg: fmt.Sprintf("`format` takes text, logfmt, json or pretty, not `%s`", s.Format)}
}
