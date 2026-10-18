package serlog

import (
	"encoding"
	"errors"
	"fmt"
	"maps"
	"math"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

// quote is v as a JSON string, escaped as every serlog package escapes it.
func quote(s string) string {
	var b strings.Builder
	b.Grow(len(s) + 2)
	b.WriteByte('"')
	for _, c := range s {
		switch c {
		case '"':
			b.WriteString(`\"`)
		case '\\':
			b.WriteString(`\\`)
		case '\n':
			b.WriteString(`\n`)
		case '\r':
			b.WriteString(`\r`)
		case '\t':
			b.WriteString(`\t`)
		default:
			if c < 0x20 {
				fmt.Fprintf(&b, `\u%04x`, c)
			} else {
				b.WriteRune(c)
			}
		}
	}
	b.WriteByte('"')
	return b.String()
}

// logfmtValue is v bare, or quoted when it is empty or holds whitespace, quotes or `=`.
func logfmtValue(v any) string {
	s := show(v)
	if s != "" && !strings.ContainsFunc(s, func(r rune) bool { return r == '"' || r == '=' || r == ' ' || r == '\t' || r == '\n' || r == '\r' }) {
		return s
	}
	return quote(s)
}

// causes lists what err wraps, then what that wraps, and so on; joined errors count once.
func causes(err error) []error {
	var out []error
	seen := map[error]bool{err: true}
	for c := errors.Unwrap(err); c != nil; c = errors.Unwrap(c) {
		if seen[c] {
			break
		}
		seen[c] = true
		out = append(out, c)
	}
	return out
}

// errorText is the error's own part of its message, without what a wrapped cause adds at the end.
func errorText(err error) string {
	msg := err.Error()
	if c := errors.Unwrap(err); c != nil {
		if trimmed, ok := strings.CutSuffix(msg, ": "+c.Error()); ok {
			return trimmed
		}
	}
	return msg
}

func sortedKeys(m reflect.Value) []reflect.Value {
	keys := m.MapKeys()
	slices.SortFunc(keys, func(a, b reflect.Value) int { return strings.Compare(fmt.Sprint(a), fmt.Sprint(b)) })
	return keys
}

// show is a value as text: `null`, `true`, `[a, b]`, `{k=v}`, errors with their causes.
func show(v any) string {
	switch x := v.(type) {
	case nil:
		return "null"
	case string:
		return x
	case error:
		parts := []string{errorText(x)}
		for _, c := range causes(x) {
			parts = append(parts, errorText(c))
		}
		return strings.Join(parts, ": ")
	case Group:
		parts := make([]string, len(x))
		for i, f := range x {
			parts[i] = f.Key + "=" + show(f.Value)
		}
		return "{" + strings.Join(parts, ", ") + "}"
	case time.Time:
		return x.Format(time.RFC3339Nano)
	case time.Duration:
		return x.String()
	case fmt.Stringer:
		return x.String()
	case encoding.TextMarshaler:
		b, _ := x.MarshalText()
		return string(b)
	case bool, int, int8, int16, int32, int64, uint, uint8, uint16, uint32, uint64:
		return fmt.Sprint(x)
	case float32:
		return strconv.FormatFloat(float64(x), 'g', -1, 32)
	case float64:
		return strconv.FormatFloat(x, 'g', -1, 64)
	}
	rv := reflect.ValueOf(v)
	switch rv.Kind() {
	case reflect.Slice, reflect.Array:
		parts := make([]string, rv.Len())
		for i := range rv.Len() {
			parts[i] = show(rv.Index(i).Interface())
		}
		return "[" + strings.Join(parts, ", ") + "]"
	case reflect.Map:
		var parts []string
		for _, k := range sortedKeys(rv) {
			parts = append(parts, fmt.Sprint(k)+"="+show(rv.MapIndex(k).Interface()))
		}
		return "{" + strings.Join(parts, ", ") + "}"
	}
	return fmt.Sprint(v)
}

func jsonFloat(f float64) string {
	if math.IsInf(f, 0) || math.IsNaN(f) {
		return quote(strconv.FormatFloat(f, 'g', -1, 64))
	}
	s := strconv.FormatFloat(f, 'f', -1, 64)
	if !strings.Contains(s, ".") {
		s += ".0"
	}
	return s
}

// jsonValue is a value as JSON.
func jsonValue(v any) string {
	switch x := v.(type) {
	case nil:
		return "null"
	case bool:
		return strconv.FormatBool(x)
	case int, int8, int16, int32, int64, uint, uint8, uint16, uint32, uint64:
		return fmt.Sprint(x)
	case float32:
		return jsonFloat(float64(x))
	case float64:
		return jsonFloat(x)
	case string:
		return quote(x)
	case error:
		out := `{"message":` + quote(errorText(x))
		if cs := causes(x); len(cs) > 0 {
			parts := make([]string, len(cs))
			for i, c := range cs {
				parts[i] = quote(errorText(c))
			}
			out += `,"causes":[` + strings.Join(parts, ",") + "]"
		}
		return out + "}"
	case Group:
		parts := make([]string, len(x))
		for i, f := range x {
			parts[i] = quote(f.Key) + ":" + jsonValue(f.Value)
		}
		return "{" + strings.Join(parts, ",") + "}"
	case time.Time:
		return quote(x.Format(time.RFC3339Nano))
	case time.Duration, fmt.Stringer, encoding.TextMarshaler:
		return quote(show(x))
	}
	rv := reflect.ValueOf(v)
	switch rv.Kind() {
	case reflect.Slice, reflect.Array:
		parts := make([]string, rv.Len())
		for i := range rv.Len() {
			parts[i] = jsonValue(rv.Index(i).Interface())
		}
		return "[" + strings.Join(parts, ",") + "]"
	case reflect.Map:
		var parts []string
		for _, k := range sortedKeys(rv) {
			parts = append(parts, quote(fmt.Sprint(k))+":"+jsonValue(rv.MapIndex(k).Interface()))
		}
		return "{" + strings.Join(parts, ",") + "}"
	}
	return quote(fmt.Sprint(v))
}

// flat is each field, maps and groups spread into `key.sub`, leaving out the keys in skip.
func flat(fields []Field, skip ...string) []Field {
	var out []Field
	var walk func(key string, v any)
	walk = func(key string, v any) {
		switch x := v.(type) {
		case Group:
			for _, f := range x {
				walk(key+"."+f.Key, f.Value)
			}
			return
		case map[string]any:
			for _, k := range slices.Sorted(maps.Keys(x)) {
				walk(key+"."+k, x[k])
			}
			return
		}
		out = append(out, Field{key, v})
	}
	for _, f := range fields {
		if !slices.Contains(skip, f.Key) {
			walk(f.Key, f.Value)
		}
	}
	return out
}

func fieldText(r *Record, quoted bool, skip ...string) string {
	var b strings.Builder
	for _, f := range flat(r.Fields, skip...) {
		b.WriteByte(' ')
		b.WriteString(f.Key)
		b.WriteByte('=')
		if quoted {
			b.WriteString(logfmtValue(f.Value))
		} else {
			b.WriteString(show(f.Value))
		}
	}
	return b.String()
}

func logfmtLine(r *Record) string {
	return "time=" + iso(r.Time, 0) + " level=" + r.Level.String() + " target=" + logfmtValue(r.Target) +
		" msg=" + logfmtValue(r.Msg) + fieldText(r, true)
}

func padRight(s string, w int) string {
	if n := utf8.RuneCountInString(s); n < w {
		return s + strings.Repeat(" ", w-n)
	}
	return s
}

func prettyLine(r *Record, t Theme) string {
	on := t != nil
	c := r.Level.Colour()
	if on {
		c = t.of(r.Level)
	}
	var b strings.Builder
	b.WriteString(ColourGrey.paint(on, writeTime("", r.Time)) + " " + c.paint(on, padRight(r.Level.Label(compat()), 5)) +
		" " + ColourWhite.paint(on, r.Target) + " " + r.Msg)
	rows := flat(r.Fields)
	width := 0
	for _, f := range rows {
		width = max(width, len(f.Key))
	}
	for _, f := range rows {
		b.WriteString("\n    " + ColourCyan.paint(on, padRight(f.Key, width)) + " = " + show(f.Value))
	}
	return b.String()
}
