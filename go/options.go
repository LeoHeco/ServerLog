package serlog

import (
	"fmt"
	"maps"
	"slices"
	"time"
)

// Options is a config entry's own keys, handed to a registered factory. Every key must be read; the first bad
// value, or a key left unread, is the error the entry fails with.
type Options struct {
	what string
	keys map[string]any
	read map[string]bool
	err  error
}

func newOptions(what string, keys map[string]any) *Options {
	return &Options{what: what, keys: keys, read: map[string]bool{}}
}

func (o *Options) fail(key, why string) {
	if o.err == nil {
		o.err = configErr("%s: `%s` %s", o.what, key, why)
	}
}

// Has is whether the key is given.
func (o *Options) Has(key string) bool {
	_, ok := o.keys[key]
	return ok
}

// Raw is the value as given, or nil.
func (o *Options) Raw(key string) any {
	o.read[key] = true
	return o.keys[key]
}

// String is a string, or "" when absent.
func (o *Options) String(key string) string {
	switch v := o.Raw(key).(type) {
	case nil:
		return ""
	case string:
		return v
	default:
		return fmt.Sprint(v)
	}
}

// Need is a string that must be given.
func (o *Options) Need(key string) string {
	s := o.String(key)
	if s == "" {
		o.fail(key, "is required")
	}
	return s
}

// Strings is a list of strings, a single string counting as one.
func (o *Options) Strings(key string) []string {
	switch v := o.Raw(key).(type) {
	case nil:
		return nil
	case string:
		return []string{v}
	case []string:
		return v
	case []any:
		out := make([]string, len(v))
		for i, x := range v {
			out[i] = fmt.Sprint(x)
		}
		return out
	}
	o.fail(key, "takes a list")
	return nil
}

// Int is a whole number, or 0 when absent.
func (o *Options) Int(key string) int {
	v := o.Raw(key)
	if v == nil {
		return 0
	}
	if f, ok := toFloat(v); ok && f == float64(int(f)) {
		return int(f)
	}
	o.fail(key, fmt.Sprintf("takes a whole number, not `%v`", v))
	return 0
}

// Float is a number, or 0 when absent.
func (o *Options) Float(key string) float64 {
	v := o.Raw(key)
	if v == nil {
		return 0
	}
	if f, ok := toFloat(v); ok {
		return f
	}
	o.fail(key, fmt.Sprintf("takes a number, not `%v`", v))
	return 0
}

// Bool is true or false, or false when absent.
func (o *Options) Bool(key string) bool {
	switch v := o.Raw(key).(type) {
	case nil:
		return false
	case bool:
		return v
	default:
		o.fail(key, fmt.Sprintf("takes true or false, not `%v`", v))
		return false
	}
}

// Duration is a duration such as `5s`, or 0 when absent.
func (o *Options) Duration(key string) time.Duration {
	v := o.Raw(key)
	if v == nil {
		return 0
	}
	d, err := duration(v)
	if err != nil {
		o.fail(key, err.Error())
	}
	return d
}

// Size is a size such as `64MiB`, or 0 when absent.
func (o *Options) Size(key string) int64 {
	v := o.Raw(key)
	if v == nil {
		return 0
	}
	n, err := size(v)
	if err != nil {
		o.fail(key, err.Error())
	}
	return n
}

// Table is a map of strings, or nil when absent.
func (o *Options) Table(key string) map[string]string {
	v := o.Raw(key)
	if v == nil {
		return nil
	}
	m, ok := v.(map[string]any)
	if !ok {
		o.fail(key, "takes a map")
		return nil
	}
	out := make(map[string]string, len(m))
	for k, x := range m {
		out[k] = fmt.Sprint(x)
	}
	return out
}

// All is every key, read or not, for factories that take free-form keys.
func (o *Options) All() map[string]any {
	for k := range o.keys {
		o.read[k] = true
	}
	return maps.Clone(o.keys)
}

// done is the first bad value, or the first key left unread.
func (o *Options) done() error {
	if o.err != nil {
		return o.err
	}
	for _, k := range slices.Sorted(maps.Keys(o.keys)) {
		if !o.read[k] {
			return configErr("%s: unknown key `%s`", o.what, k)
		}
	}
	return nil
}
