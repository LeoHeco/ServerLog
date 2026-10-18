package serlog

import (
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// Field is a named value attached to a record. Values keep their types: nil, booleans, numbers, strings, slices,
// maps, groups and errors are shown and serialised as such; anything else as its string.
type Field struct {
	Key   string
	Value any
}

// Group is a set of fields in order, as a field's value; slog groups become one.
type Group []Field

// Record is one log event as every sink sees it. Treat it as read-only: a processor that changes one returns a
// copy, so every other slot still sees the original.
type Record struct {
	Level  Level
	Target string
	Msg    string
	Fields []Field
	Tags   []string
	Module string
	Func   string
	File   string
	Line   int
	Thread string
	Spans  string
	Time   time.Time
	Seq    uint64
}

var seq atomic.Uint64

// NewRecord makes a record now.
func NewRecord(level Level, target, msg string, fields ...Field) *Record {
	return &Record{Level: level, Target: target, Msg: msg, Fields: fields, Time: time.Now(), Seq: seq.Add(1) - 1}
}

// Field is the last field called key.
func (r *Record) Field(key string) (any, bool) {
	for i := len(r.Fields) - 1; i >= 0; i-- {
		if r.Fields[i].Key == key {
			return r.Fields[i].Value, true
		}
	}
	return nil, false
}

// Clone is a copy that can be changed without touching r.
func (r *Record) Clone() *Record {
	c := *r
	c.Fields = append([]Field(nil), r.Fields...)
	c.Tags = append([]string(nil), r.Tags...)
	return &c
}

// With is a copy with these fields set, replacing fields of the same name.
func (r *Record) With(set ...Field) *Record {
	c := *r
	c.Fields = make([]Field, 0, len(r.Fields)+len(set))
	for _, f := range r.Fields {
		keep := true
		for _, s := range set {
			keep = keep && s.Key != f.Key
		}
		if keep {
			c.Fields = append(c.Fields, f)
		}
	}
	c.Fields = append(c.Fields, set...)
	return &c
}

// Meta is who is logging, attached to records that leave the process.
type Meta struct {
	App, Env, Instance string
}

var hostName = sync.OnceValue(func() string {
	for _, v := range []string{os.Getenv("HOSTNAME"), os.Getenv("COMPUTERNAME")} {
		if v != "" {
			return v
		}
	}
	h, _ := os.Hostname()
	return h
})

func detectMeta() Meta {
	exe := filepath.Base(os.Args[0])
	return Meta{App: strings.TrimSuffix(exe, filepath.Ext(exe)), Instance: hostName()}
}
