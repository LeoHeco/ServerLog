package serlog

import (
	"regexp"
	"strconv"
	"time"
)

var (
	born      = time.Now()
	directive = regexp.MustCompile(`%(:z|\.\d+f|.)`)
)

func pad(n, w int) string {
	s := strconv.Itoa(n)
	for len(s) < w {
		s = "0" + s
	}
	return s
}

func offset(t time.Time, colon bool) string {
	_, secs := t.Zone()
	sign := "+"
	if secs < 0 {
		sign, secs = "-", -secs
	}
	m := secs / 60
	sep := ""
	if colon {
		sep = ":"
	}
	return sign + pad(m/60, 2) + sep + pad(m%60, 2)
}

func fraction(t time.Time, digits int) string {
	return "." + pad(t.Nanosecond(), 9)[:min(digits, 9)]
}

// strftime formats `%Y %y %m %d %H %M %S %j %F %T %z %:z %G %V %%` and `%.3f`, `%.6f`, `%.9f` in t's zone;
// other directives pass through.
func strftime(t time.Time, f string) string {
	return directive.ReplaceAllStringFunc(f, func(m string) string {
		switch d := m[1:]; d {
		case "Y":
			return strconv.Itoa(t.Year())
		case "y":
			return pad(t.Year()%100, 2)
		case "m":
			return pad(int(t.Month()), 2)
		case "d":
			return pad(t.Day(), 2)
		case "H":
			return pad(t.Hour(), 2)
		case "M":
			return pad(t.Minute(), 2)
		case "S":
			return pad(t.Second(), 2)
		case "j":
			return pad(t.YearDay(), 3)
		case "F":
			return strftime(t, "%Y-%m-%d")
		case "T":
			return strftime(t, "%H:%M:%S")
		case "z":
			return offset(t, false)
		case ":z":
			return offset(t, true)
		case "G":
			y, _ := t.ISOWeek()
			return strconv.Itoa(y)
		case "V":
			_, w := t.ISOWeek()
			return pad(w, 2)
		case "%":
			return "%"
		default:
			if d[0] == '.' {
				n, _ := strconv.Atoi(d[1 : len(d)-1])
				return fraction(t, n)
			}
			return m
		}
	})
}

// iso is UTC RFC 3339 with `digits` decimals: `2023-11-14T22:13:20.000Z`.
func iso(t time.Time, digits int) string {
	u := t.UTC()
	base := strftime(u, "%Y-%m-%dT%H:%M:%S")
	if digits > 0 {
		return base + fraction(u, digits) + "Z"
	}
	return base + "Z"
}

func writeTime(spec string, t time.Time) string {
	switch spec {
	case "", "local":
		return strftime(t.Local(), "%Y-%m-%d %H:%M:%S%.3f")
	case "utc":
		return iso(t, 0)
	case "rfc3339":
		return strftime(t.Local(), "%Y-%m-%dT%H:%M:%S%.6f%:z")
	case "unix":
		return strconv.FormatInt(t.Unix(), 10)
	case "unix_ms":
		return strconv.FormatInt(t.UnixMilli(), 10)
	case "unix_ns":
		return strconv.FormatInt(t.UnixNano(), 10)
	}
	return strftime(t.Local(), spec)
}

func elapsed() int64 { return time.Since(born).Milliseconds() }
