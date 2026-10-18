// Package tree writes ordered config trees as JSON, YAML and TOML.
package tree

import (
	"fmt"
	"maps"
	"math"
	"reflect"
	"regexp"
	"slices"
	"strconv"
	"strings"

	"go.yaml.in/yaml/v3"
)

// KV is one key and its value.
type KV struct {
	Key   string
	Value any
}

// Map is a map that keeps its keys in order.
type Map []KV

// Set appends a key, or replaces its value if it is there.
func (m *Map) Set(k string, v any) {
	for i := range *m {
		if (*m)[i].Key == k {
			(*m)[i].Value = v
			return
		}
	}
	*m = append(*m, KV{k, v})
}

// pairs is v's entries in order when v is a map of either kind.
func pairs(v any) (Map, bool) {
	switch x := v.(type) {
	case Map:
		return x, true
	case map[string]any:
		out := make(Map, 0, len(x))
		for _, k := range slices.Sorted(maps.Keys(x)) {
			out = append(out, KV{k, x[k]})
		}
		return out, true
	case map[string]string:
		out := make(Map, 0, len(x))
		for _, k := range slices.Sorted(maps.Keys(x)) {
			out = append(out, KV{k, x[k]})
		}
		return out, true
	}
	return nil, false
}

// items is v's elements when v is a slice.
func items(v any) ([]any, bool) {
	if v == nil {
		return nil, false
	}
	rv := reflect.ValueOf(v)
	if rv.Kind() != reflect.Slice || rv.Type() == reflect.TypeFor[Map]() {
		return nil, false
	}
	out := make([]any, rv.Len())
	for i := range out {
		out[i] = rv.Index(i).Interface()
	}
	return out, true
}

func quote(s string) string {
	var b strings.Builder
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

func float(f float64) string {
	if math.IsInf(f, 0) || math.IsNaN(f) {
		return quote(strconv.FormatFloat(f, 'g', -1, 64))
	}
	s := strconv.FormatFloat(f, 'f', -1, 64)
	if !strings.Contains(s, ".") {
		s += ".0"
	}
	return s
}

func scalar(v any) string {
	switch x := v.(type) {
	case nil:
		return "null"
	case string:
		return quote(x)
	case bool:
		return strconv.FormatBool(x)
	case float64:
		return float(x)
	case float32:
		return float(float64(x))
	}
	return fmt.Sprint(v)
}

// JSON is v as indented JSON, keys in order.
func JSON(v any) string {
	var b strings.Builder
	writeJSON(&b, v, "")
	b.WriteByte('\n')
	return b.String()
}

func writeJSON(b *strings.Builder, v any, indent string) {
	in := indent + "  "
	if m, ok := pairs(v); ok {
		if len(m) == 0 {
			b.WriteString("{}")
			return
		}
		b.WriteString("{\n")
		for i, kv := range m {
			b.WriteString(in + quote(kv.Key) + ": ")
			writeJSON(b, kv.Value, in)
			if i < len(m)-1 {
				b.WriteByte(',')
			}
			b.WriteByte('\n')
		}
		b.WriteString(indent + "}")
		return
	}
	if xs, ok := items(v); ok {
		if len(xs) == 0 {
			b.WriteString("[]")
			return
		}
		b.WriteString("[\n")
		for i, x := range xs {
			b.WriteString(in)
			writeJSON(b, x, in)
			if i < len(xs)-1 {
				b.WriteByte(',')
			}
			b.WriteByte('\n')
		}
		b.WriteString(indent + "]")
		return
	}
	b.WriteString(scalar(v))
}

func node(v any) *yaml.Node {
	if m, ok := pairs(v); ok {
		n := &yaml.Node{Kind: yaml.MappingNode}
		for _, kv := range m {
			n.Content = append(n.Content, &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: kv.Key}, node(kv.Value))
		}
		return n
	}
	if xs, ok := items(v); ok {
		n := &yaml.Node{Kind: yaml.SequenceNode}
		for _, x := range xs {
			n.Content = append(n.Content, node(x))
		}
		return n
	}
	switch x := v.(type) {
	case nil:
		return &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!null", Value: "null"}
	case string:
		return &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: x}
	case bool:
		return &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!bool", Value: strconv.FormatBool(x)}
	case float64, float32:
		return &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!float", Value: scalar(x)}
	}
	return &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!int", Value: fmt.Sprint(v)}
}

// YAML is v as block YAML, keys in order.
func YAML(v any) (string, error) {
	out, err := yaml.Marshal(node(v))
	return string(out), err
}

var bare = regexp.MustCompile(`^[A-Za-z0-9_-]+$`)

func key(k string) string {
	if bare.MatchString(k) {
		return k
	}
	return quote(k)
}

func inline(v any) string {
	if m, ok := pairs(v); ok {
		parts := make([]string, len(m))
		for i, kv := range m {
			parts[i] = key(kv.Key) + " = " + inline(kv.Value)
		}
		if len(parts) == 0 {
			return "{}"
		}
		return "{ " + strings.Join(parts, ", ") + " }"
	}
	if xs, ok := items(v); ok {
		parts := make([]string, len(xs))
		for i, x := range xs {
			parts[i] = inline(x)
		}
		return "[" + strings.Join(parts, ", ") + "]"
	}
	return scalar(v)
}

func tables(xs []any) bool {
	if len(xs) == 0 {
		return false
	}
	for _, x := range xs {
		if _, ok := pairs(x); !ok {
			return false
		}
	}
	return true
}

// TOML is m as TOML: plain keys first, then tables, then arrays of tables; anything deeper is written inline.
func TOML(m Map) string {
	var b strings.Builder
	writeTable(&b, m, "")
	return b.String()
}

func writeTable(b *strings.Builder, m Map, path string) {
	var subs, arrays Map
	for _, kv := range m {
		if inner, ok := pairs(kv.Value); ok {
			subs = append(subs, KV{kv.Key, inner})
			continue
		}
		if xs, ok := items(kv.Value); ok && tables(xs) {
			arrays = append(arrays, kv)
			continue
		}
		b.WriteString(key(kv.Key) + " = " + inline(kv.Value) + "\n")
	}
	for _, kv := range subs {
		p := join(path, kv.Key)
		b.WriteString("\n[" + p + "]\n")
		writeTable(b, kv.Value.(Map), p)
	}
	for _, kv := range arrays {
		p := join(path, kv.Key)
		xs, _ := items(kv.Value)
		for _, x := range xs {
			b.WriteString("\n[[" + p + "]]\n")
			entry, _ := pairs(x)
			for _, e := range entry {
				b.WriteString(key(e.Key) + " = " + inline(e.Value) + "\n")
			}
		}
	}
}

func join(path, k string) string {
	if path == "" {
		return key(k)
	}
	return path + "." + key(k)
}
