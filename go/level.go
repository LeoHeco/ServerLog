package serlog

import (
	"fmt"
	"log/slog"
	"regexp"
	"strconv"
	"strings"
	"sync"
)

// Level is a severity: six built-in levels from 10 to 60, plus any defined at run time.
// Levels compare by value only.
type Level uint8

// The built-in levels.
const (
	LevelTrace Level = 10
	LevelDebug Level = 20
	LevelInfo  Level = 30
	LevelWarn  Level = 40
	LevelError Level = 50
	LevelFatal Level = 60
)

type levelInfo struct {
	name   string
	colour Colour
}

var (
	builtIn = map[Level]levelInfo{
		LevelTrace: {"trace", ColourCyan},
		LevelDebug: {"debug", ColourMagenta},
		LevelInfo:  {"info", ColourGreen},
		LevelWarn:  {"warn", ColourYellow},
		LevelError: {"error", ColourRed},
		LevelFatal: {"fatal", ColourAlarm},
	}
	aliases   = map[string]Level{"warning": LevelWarn, "critical": LevelFatal}
	customMu  sync.RWMutex
	custom    = map[Level]levelInfo{}
	levelName = regexp.MustCompile(`^[a-z0-9_]+$`)
)

func lookup(l Level) (levelInfo, bool) {
	if i, ok := builtIn[l]; ok {
		return i, true
	}
	customMu.RLock()
	defer customMu.RUnlock()
	i, ok := custom[l]
	return i, ok
}

// ParseLevel reads a level from a name in any case, `warning` and `critical` included, or from digits.
func ParseLevel(s string) (Level, error) {
	n := strings.ToLower(strings.TrimSpace(s))
	if v, err := strconv.Atoi(n); err == nil {
		if v < 0 || v > 255 {
			return 0, fmt.Errorf("a level is 0 to 255, not %d", v)
		}
		return Level(v), nil
	}
	if l, ok := aliases[n]; ok {
		return l, nil
	}
	for l, i := range builtIn {
		if i.name == n {
			return l, nil
		}
	}
	customMu.RLock()
	defer customMu.RUnlock()
	for l, i := range custom {
		if i.name == n {
			return l, nil
		}
	}
	return 0, fmt.Errorf("unknown level %q: expected trace, debug, info, warn, error, fatal, a defined level or a number", s)
}

// DefineLevel defines a level, or renames the one with that value.
func DefineLevel(name string, value Level, colour Colour) error {
	_, alias := aliases[name]
	taken := false
	for _, i := range builtIn {
		taken = taken || i.name == name
	}
	if !levelName.MatchString(name) || name == "off" || alias || taken {
		return fmt.Errorf("bad level name %q", name)
	}
	customMu.Lock()
	defer customMu.Unlock()
	for l, i := range custom {
		if i.name == name {
			delete(custom, l)
		}
	}
	custom[value] = levelInfo{name, colour}
	return nil
}

// String is the lower-case name used in config files, filters and labels; `custom` for an unnamed value.
func (l Level) String() string {
	if i, ok := lookup(l); ok {
		return i.name
	}
	return "custom"
}

// Colour is the level's colour in a terminal.
func (l Level) Colour() Colour {
	if i, ok := lookup(l); ok {
		return i.colour
	}
	return ColourPlain
}

// Label is how a profile prints the level, such as `WARNING` for Python's.
func (l Level) Label(c Compat) string {
	switch {
	case c == CompatPython && l == LevelWarn:
		return "WARNING"
	case c == CompatPython && l == LevelFatal:
		return "CRITICAL"
	case c == CompatSlog && l == LevelTrace:
		return "DEBUG-4"
	case c == CompatSlog && l == LevelFatal:
		return "ERROR+4"
	}
	return strings.ToUpper(l.String())
}

// Slog is the level as log/slog numbers it: trace is DEBUG-4, fatal ERROR+4.
func (l Level) Slog() slog.Level {
	switch {
	case l <= LevelTrace:
		return slog.LevelDebug - 4
	case l <= LevelDebug:
		return slog.LevelDebug
	case l <= LevelInfo:
		return slog.LevelInfo
	case l <= LevelWarn:
		return slog.LevelWarn
	case l <= LevelError:
		return slog.LevelError
	}
	return slog.LevelError + 4
}

// FromSlog is the serlog level for a log/slog level.
func FromSlog(l slog.Level) Level {
	switch {
	case l < slog.LevelDebug:
		return LevelTrace
	case l < slog.LevelInfo:
		return LevelDebug
	case l < slog.LevelWarn:
		return LevelInfo
	case l < slog.LevelError:
		return LevelWarn
	case l < slog.LevelError+4:
		return LevelError
	}
	return LevelFatal
}

func (l Level) syslog() int {
	switch {
	case l <= LevelDebug:
		return 7
	case l <= LevelInfo:
		return 6
	case l <= LevelWarn:
		return 4
	case l <= LevelError:
		return 3
	}
	return 2
}

func definedLevels() map[Level]levelInfo {
	customMu.RLock()
	defer customMu.RUnlock()
	out := make(map[Level]levelInfo, len(custom))
	for l, i := range custom {
		out[l] = i
	}
	return out
}
