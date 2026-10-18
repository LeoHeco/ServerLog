package serlog

import (
	"context"
	"log/slog"
	"runtime"
	"strings"
	"time"
)

// handler is serlog as a log/slog handler: attributes become fields, groups nest, `tags` lists tags, and a
// logger's target is fixed when it is made.
type handler struct {
	target string
	fields []Field
	open   []group
}

type group struct {
	name   string
	fields []Field
}

// Handler is a log/slog handler that writes through serlog under target.
func Handler(target string) slog.Handler { return &handler{target: target} }

// Get is a logger for target, with fields given as slog key-value pairs or attributes.
func Get(target string, args ...any) *slog.Logger { return slog.New(Handler(target)).With(args...) }

func (h *handler) Enabled(_ context.Context, l slog.Level) bool { return Enabled(FromSlog(l)) }

func value(v slog.Value) any {
	v = v.Resolve()
	switch v.Kind() {
	case slog.KindString:
		return v.String()
	case slog.KindInt64:
		return v.Int64()
	case slog.KindUint64:
		return v.Uint64()
	case slog.KindFloat64:
		return v.Float64()
	case slog.KindBool:
		return v.Bool()
	case slog.KindDuration:
		return v.Duration()
	case slog.KindTime:
		return v.Time()
	case slog.KindGroup:
		return Group(fields(v.Group()))
	}
	return v.Any()
}

// fields turns attributes into fields: empty ones are left out, inline groups spread, empty groups dropped.
func fields(as []slog.Attr) []Field {
	var out []Field
	for _, a := range as {
		a.Value = a.Value.Resolve()
		if a.Equal(slog.Attr{}) {
			continue
		}
		if a.Value.Kind() == slog.KindGroup {
			inner := fields(a.Value.Group())
			if len(inner) == 0 {
				continue
			}
			if a.Key == "" {
				out = append(out, inner...)
				continue
			}
			out = append(out, Field{a.Key, Group(inner)})
			continue
		}
		out = append(out, Field{a.Key, value(a.Value)})
	}
	return out
}

func (h *handler) WithAttrs(as []slog.Attr) slog.Handler {
	f := fields(as)
	if len(f) == 0 {
		return h
	}
	out := *h
	if n := len(h.open); n > 0 {
		out.open = append([]group(nil), h.open...)
		out.open[n-1].fields = append(append([]Field(nil), h.open[n-1].fields...), f...)
	} else {
		out.fields = append(append([]Field(nil), h.fields...), f...)
	}
	return &out
}

func (h *handler) WithGroup(name string) slog.Handler {
	if name == "" {
		return h
	}
	out := *h
	out.open = append(append([]group(nil), h.open...), group{name: name})
	return &out
}

// splitCaller reads `pkg/path.(*T).Method` as the package path and the function.
func splitCaller(fn string) (string, string) {
	slash := strings.LastIndexByte(fn, '/') + 1
	if dot := strings.IndexByte(fn[slash:], '.'); dot >= 0 {
		return fn[:slash+dot], fn[slash+dot+1:]
	}
	return "", fn
}

func (h *handler) Handle(ctx context.Context, sr slog.Record) error {
	h.handle(ctx, sr, FromSlog(sr.Level))
	return nil
}

func (h *handler) handle(ctx context.Context, sr slog.Record, level Level) {
	var call []Field
	var tags []string
	sr.Attrs(func(a slog.Attr) bool {
		if a.Key == "tags" && len(h.open) == 0 {
			switch t := a.Value.Resolve().Any().(type) {
			case []string:
				tags = append(tags, t...)
				return true
			case string:
				tags = append(tags, t)
				return true
			}
		}
		call = append(call, fields([]slog.Attr{a})...)
		return true
	})
	for i := len(h.open) - 1; i >= 0; i-- {
		g := append(append([]Field(nil), h.open[i].fields...), call...)
		call = nil
		if len(g) > 0 {
			call = []Field{{h.open[i].name, Group(g)}}
		}
	}
	all := append(append(bound(ctx), h.fields...), call...)
	for i, f := range all {
		if f.Key == "err" {
			if _, ok := f.Value.(error); ok {
				all[i].Key = "error"
			}
		}
	}
	r := &Record{Level: level, Target: h.target, Msg: sr.Message, Fields: all, Tags: tags, Time: sr.Time, Seq: seq.Add(1) - 1}
	if sr.PC != 0 && load().caller {
		f, _ := runtime.CallersFrames([]uintptr{sr.PC}).Next()
		r.Module, r.Func = splitCaller(f.Function)
		r.File, r.Line = f.File, f.Line
	}
	emit(ctx, r)
}

type scopeKey struct{}

// Bind is ctx with fields added to every record logged with it, given as slog key-value pairs or attributes.
func Bind(ctx context.Context, args ...any) context.Context {
	r := slog.NewRecord(time.Time{}, 0, "", 0)
	r.Add(args...)
	var as []slog.Attr
	r.Attrs(func(a slog.Attr) bool {
		as = append(as, a)
		return true
	})
	return context.WithValue(ctx, scopeKey{}, append(bound(ctx), fields(as)...))
}

func bound(ctx context.Context) []Field {
	if ctx == nil {
		return nil
	}
	f, _ := ctx.Value(scopeKey{}).([]Field)
	return append([]Field(nil), f...)
}

func root() string { return or(meta().App, "main") }

// Log logs through the app's logger at level, with slog key-value pairs or attributes.
func Log(ctx context.Context, level Level, msg string, args ...any) { at(ctx, level, msg, args) }

func at(ctx context.Context, level Level, msg string, args []any) {
	if !Enabled(level) {
		return
	}
	var pcs [1]uintptr
	runtime.Callers(3, pcs[:])
	r := slog.NewRecord(time.Now(), level.Slog(), msg, pcs[0])
	r.Add(args...)
	(&handler{target: root()}).handle(ctx, r, level)
}

// Trace logs at trace through the app's logger.
func Trace(msg string, args ...any) { at(context.Background(), LevelTrace, msg, args) }

// Debug logs at debug through the app's logger.
func Debug(msg string, args ...any) { at(context.Background(), LevelDebug, msg, args) }

// Info logs at info through the app's logger.
func Info(msg string, args ...any) { at(context.Background(), LevelInfo, msg, args) }

// Warn logs at warn through the app's logger.
func Warn(msg string, args ...any) { at(context.Background(), LevelWarn, msg, args) }

// Error logs at error through the app's logger.
func Error(msg string, args ...any) { at(context.Background(), LevelError, msg, args) }

// Fatal logs at fatal through the app's logger; it does not exit.
func Fatal(msg string, args ...any) { at(context.Background(), LevelFatal, msg, args) }

// Err is the `error` attribute.
func Err(err error) slog.Attr { return slog.Any("error", err) }

// Tags is the `tags` attribute, which becomes the record's tags.
func Tags(tags ...string) slog.Attr { return slog.Any("tags", tags) }

// Capture makes serlog slog's default handler, so slog's top-level calls and the log package write through it.
func Capture() { slog.SetDefault(Get(root())) }
