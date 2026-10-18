package serlog

import (
	"errors"
	"fmt"
)

var (
	// ErrTwice is returned when Init runs a second time.
	ErrTwice = errors.New("serlog is already initialised")
	// ErrRejected marks errors where sending again will not help; sinks wrap it with %w.
	ErrRejected = errors.New("rejected")
)

// ConfigError says a setting is missing or malformed.
type ConfigError struct {
	Msg string
	Err error
}

func (e *ConfigError) Error() string { return e.Msg }
func (e *ConfigError) Unwrap() error { return e.Err }

func configErr(format string, args ...any) error {
	return &ConfigError{Msg: fmt.Sprintf(format, args...)}
}

// KindError says a config names a kind nobody registered.
type KindError struct{ What, Name string }

func (e *KindError) Error() string { return fmt.Sprintf("unknown %s kind `%s`", e.What, e.Name) }

// NameTakenError says two sinks share a name, or a name is reserved.
type NameTakenError struct{ Name string }

func (e *NameTakenError) Error() string {
	return fmt.Sprintf("sink name `%s` is taken or reserved", e.Name)
}

// ClaimedError says a resource, such as a file, is already claimed by another sink.
type ClaimedError struct{ Resource string }

func (e *ClaimedError) Error() string { return e.Resource + " is already used by another sink" }

// NotReadyError says a sink failed its start-up check.
type NotReadyError struct {
	Sink string
	Err  error
}

func (e *NotReadyError) Error() string {
	return fmt.Sprintf("sink `%s` is not ready: %v", e.Sink, e.Err)
}
func (e *NotReadyError) Unwrap() error { return e.Err }

// SinkError is what the panic policy panics with when a sink fails.
type SinkError struct {
	Sink string
	Err  error
}

func (e *SinkError) Error() string { return fmt.Sprintf("sink `%s` failed: %v", e.Sink, e.Err) }
func (e *SinkError) Unwrap() error { return e.Err }

func retryable(err error) bool {
	var c *ConfigError
	return !errors.Is(err, ErrRejected) && !errors.As(err, &c)
}

func message(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}
