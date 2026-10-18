package serlog

import (
	"fmt"
	"strings"
)

// Compat is a profile that makes serlog behave like another logger: its pattern, console stream, default level
// and the environment variables it reads for the filter. `log4j` is another name for `logback`.
type Compat string

// The profiles.
const (
	CompatSerlog    Compat = "serlog"
	CompatTracing   Compat = "tracing"
	CompatEnvLogger Compat = "env_logger"
	CompatPython    Compat = "python"
	CompatLogback   Compat = "logback"
	CompatSlog      Compat = "slog"
	CompatPino      Compat = "pino"
)

var patterns = map[Compat]string{
	CompatSerlog:    "{time} {level:<5} {target}: {msg}{fields}",
	CompatTracing:   "{time:rfc3339} {level:>5} {spans}{target}: {msg}{fields}",
	CompatEnvLogger: "[{time:utc} {level:<5} {target}] {msg}",
	CompatPython:    "{level}:{target}:{msg}",
	CompatLogback:   "{time:%H:%M:%S%.3f} [{thread}] {level:<5} {target:~36} - {msg}",
	CompatSlog:      "time={time:%Y-%m-%dT%H:%M:%S%.3f%:z} level={level} msg={msg:q}{fields:q}",
	CompatPino:      "[{time:%H:%M:%S%.3f}] {level} ({pid}): {msg}{fields}",
}

// ParseCompat reads a profile by its config name.
func ParseCompat(s string) (Compat, error) {
	n := Compat(strings.ToLower(strings.TrimSpace(s)))
	if n == "log4j" {
		n = CompatLogback
	}
	if _, ok := patterns[n]; !ok {
		return "", fmt.Errorf("unknown compat profile %q", s)
	}
	return n, nil
}

// Pattern is the pattern that reproduces the logger's default output.
func (c Compat) Pattern() string { return patterns[c] }

// Stream is where its console writes.
func (c Compat) Stream() string {
	if c == CompatEnvLogger || c == CompatPython {
		return "stderr"
	}
	return "stdout"
}

// Level is its default level.
func (c Compat) Level() Level {
	switch c {
	case CompatEnvLogger:
		return LevelError
	case CompatPython:
		return LevelWarn
	case CompatLogback:
		return LevelDebug
	}
	return LevelInfo
}

// Env lists the environment variables it reads for the filter.
func (c Compat) Env() []string {
	switch c {
	case CompatSerlog:
		return []string{"SERLOG_FILTER"}
	case CompatTracing, CompatEnvLogger:
		return []string{"RUST_LOG"}
	}
	return nil
}
