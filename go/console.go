package serlog

import (
	"io"
	"os"
	"strings"
)

// Console prints one line per entry. Stream is "stdout", "stderr" or "auto", which sends error and above to
// standard error; the profile chooses when it is empty.
type Console struct {
	Slot
	Stream string
}

func (c *Console) out(l Level) *os.File {
	s := or(c.Stream, compat().Stream())
	if s == "stderr" || s == "auto" && l >= LevelError {
		return os.Stderr
	}
	return os.Stdout
}

// Kind is "console".
func (c *Console) Kind() string { return "console" }

// Ready checks the stream.
func (c *Console) Ready() error {
	switch c.Stream {
	case "", "stdout", "stderr", "auto":
		return nil
	}
	return configErr("console: `stream` is stdout, stderr or auto, not `%s`", c.Stream)
}

// Emit prints the line.
func (c *Console) Emit(e *Entry) error {
	_, err := io.WriteString(c.out(e.Record.Level), e.Line()+"\n")
	return err
}

// EmitBatch prints a batch with one write per stream.
func (c *Console) EmitBatch(es []*Entry) error {
	for _, f := range []*os.File{os.Stdout, os.Stderr} {
		var b strings.Builder
		for _, e := range es {
			if c.out(e.Record.Level) == f {
				b.WriteString(e.Line() + "\n")
			}
		}
		if b.Len() > 0 {
			if _, err := io.WriteString(f, b.String()); err != nil {
				return err
			}
		}
	}
	return nil
}

// TTY is whether the stream is a terminal.
func (c *Console) TTY() bool {
	st, err := c.out(LevelInfo).Stat()
	return err == nil && st.Mode()&os.ModeCharDevice != 0
}

// Options describes the sink for export.
func (c *Console) Options() map[string]any {
	if c.Stream == "" {
		return map[string]any{}
	}
	return map[string]any{"stream": c.Stream}
}
