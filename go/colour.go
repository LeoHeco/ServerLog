package serlog

import (
	"fmt"
	"os"
	"strings"
)

// Colour is a terminal colour.
type Colour uint8

// The colours, by their config names.
const (
	ColourPlain Colour = iota
	ColourRed
	ColourGreen
	ColourYellow
	ColourBlue
	ColourMagenta
	ColourCyan
	ColourWhite
	ColourGrey
	ColourAlarm
)

var colourNames = [...]string{"plain", "red", "green", "yellow", "blue", "magenta", "cyan", "white", "grey", "alarm"}

var ansi = [...]string{"", "\x1b[1;31m", "\x1b[32m", "\x1b[33m", "\x1b[34m", "\x1b[35m", "\x1b[36m", "\x1b[37m", "\x1b[90m", "\x1b[1;37;41m"}

// ParseColour reads a colour by its config name; `gray` means `grey`.
func ParseColour(s string) (Colour, error) {
	n := strings.ToLower(strings.TrimSpace(s))
	if n == "gray" {
		n = "grey"
	}
	for i, name := range colourNames {
		if name == n {
			return Colour(i), nil
		}
	}
	return 0, fmt.Errorf("unknown colour %q", s)
}

func (c Colour) String() string { return colourNames[c] }

func (c Colour) paint(on bool, s string) string {
	if !on || c == ColourPlain {
		return s
	}
	return ansi[c] + s + "\x1b[0m"
}

// Theme gives some levels other colours; the rest keep their own.
type Theme map[Level]Colour

func (t Theme) of(l Level) Colour {
	if c, ok := t[l]; ok {
		return c
	}
	return l.Colour()
}

// colourOn decides `auto`, `always` or `never` for a sink that is, or is not, a terminal.
func colourOn(when string, tty bool) (bool, error) {
	switch when {
	case "", "auto":
		if os.Getenv("FORCE_COLOR") != "" {
			return true, nil
		}
		return tty && os.Getenv("NO_COLOR") == "", nil
	case "always":
		return true, nil
	case "never":
		return false, nil
	}
	return false, fmt.Errorf("`colour` is auto, always or never, not %q", when)
}
