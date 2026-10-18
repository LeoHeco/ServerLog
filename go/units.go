package serlog

import (
	"fmt"
	"math"
	"regexp"
	"strconv"
	"strings"
	"time"
)

var number = regexp.MustCompile(`^\s*(\d+(?:\.\d+)?)\s*([a-zA-Z]*)\s*$`)

var timeUnits = map[string]float64{"ms": 0.001, "": 1, "s": 1, "m": 60, "h": 3600, "d": 86400}

var sizeUnits = map[string]float64{
	"": 1, "b": 1, "k": 1e3, "kb": 1e3, "kib": 1 << 10, "m": 1e6, "mb": 1e6, "mib": 1 << 20,
	"g": 1e9, "gb": 1e9, "gib": 1 << 30,
}

// duration reads seconds, a time.Duration, or text with `ms`, `s`, `m`, `h` or `d`.
func duration(v any) (time.Duration, error) {
	switch x := v.(type) {
	case time.Duration:
		return x, nil
	case string:
		m := number.FindStringSubmatch(x)
		if m == nil {
			return 0, fmt.Errorf("bad duration %q", x)
		}
		unit, ok := timeUnits[m[2]]
		if !ok {
			return 0, fmt.Errorf("bad duration %q", x)
		}
		f, _ := strconv.ParseFloat(m[1], 64)
		return time.Duration(math.Round(f * unit * 1e9)), nil
	}
	if f, ok := toFloat(v); ok {
		if f < 0 {
			return 0, fmt.Errorf("a duration cannot be negative: %v", f)
		}
		return time.Duration(math.Round(f * 1e9)), nil
	}
	return 0, fmt.Errorf("bad duration %v", v)
}

// size reads a byte count: a number, or text such as `500KB` or `64MiB`.
func size(v any) (int64, error) {
	if s, ok := v.(string); ok {
		m := number.FindStringSubmatch(s)
		if m == nil {
			return 0, fmt.Errorf("bad size %q", s)
		}
		unit, ok := sizeUnits[strings.ToLower(m[2])]
		if !ok {
			return 0, fmt.Errorf("bad size %q", s)
		}
		f, _ := strconv.ParseFloat(m[1], 64)
		return int64(math.Floor(f * unit)), nil
	}
	if f, ok := toFloat(v); ok {
		return int64(f), nil
	}
	return 0, fmt.Errorf("bad size %v", v)
}

func toFloat(v any) (float64, bool) {
	switch x := v.(type) {
	case int:
		return float64(x), true
	case int64:
		return float64(x), true
	case uint64:
		return float64(x), true
	case float64:
		return x, true
	}
	return 0, false
}

// showTime is the shortest way to write a duration that reads back the same.
func showTime(d time.Duration) string {
	ms := d.Milliseconds()
	for _, u := range []struct {
		n    int64
		unit string
	}{{86_400_000, "d"}, {3_600_000, "h"}, {60_000, "m"}, {1000, "s"}} {
		if ms >= u.n && ms%u.n == 0 {
			return strconv.FormatInt(ms/u.n, 10) + u.unit
		}
	}
	return strconv.FormatInt(ms, 10) + "ms"
}

// showSize is the shortest way to write a byte count that reads back the same.
func showSize(n int64) any {
	for _, u := range []struct {
		n    int64
		unit string
	}{{1 << 30, "GiB"}, {1 << 20, "MiB"}, {1 << 10, "KiB"}} {
		if n >= u.n && n%u.n == 0 {
			return strconv.FormatInt(n/u.n, 10) + u.unit
		}
	}
	return n
}
