package serlog

import (
	"fmt"
	"slices"
	"strconv"
	"strings"
)

// Schemas lists the JSON shapes: serlog's own, pino, bunyan, zap, slog, structlog, logstash, ECS, GELF and the
// OpenTelemetry log model.
var Schemas = []string{"serlog", "pino", "bunyan", "zap", "slog", "structlog", "logstash", "ecs", "gelf", "otel"}

// JSON lays records out as JSON lines in a schema. Keys renames any key, Levels writes levels as "name" or
// "number", and Nest puts the fields under one key.
type JSON struct {
	Schema string
	Keys   map[string]string
	Levels string
	Nest   string
}

type pair struct{ key, json string }

func (j *JSON) check() error {
	if j.Schema != "" && !slices.Contains(Schemas, j.Schema) {
		return fmt.Errorf("unknown JSON schema `%s`", j.Schema)
	}
	if j.Levels != "" && j.Levels != "name" && j.Levels != "number" {
		return fmt.Errorf("`levels` is name or number, not `%s`", j.Levels)
	}
	return nil
}

func (j *JSON) object(pairs []pair) string {
	var b strings.Builder
	b.WriteByte('{')
	for i, p := range pairs {
		if i > 0 {
			b.WriteByte(',')
		}
		k := p.key
		if n, ok := j.Keys[k]; ok {
			k = n
		}
		b.WriteString(quote(k) + ":" + p.json)
	}
	b.WriteByte('}')
	return b.String()
}

func (j *JSON) level(r *Record, name string, numeric bool) string {
	if j.Levels == "number" || j.Levels == "" && numeric {
		return strconv.Itoa(int(r.Level))
	}
	return quote(name)
}

func (j *JSON) fields(r *Record, errKey string) []pair {
	pairs := make([]pair, len(r.Fields))
	for i, f := range r.Fields {
		k := f.Key
		if k == "error" {
			k = errKey
		}
		pairs[i] = pair{k, jsonValue(f.Value)}
	}
	if j.Nest != "" && len(pairs) > 0 {
		return []pair{{j.Nest, j.object(pairs)}}
	}
	return pairs
}

func seconds(r *Record) string {
	ms := r.Time.UnixMilli()
	return fmt.Sprintf("%d.%03d", ms/1000, ms%1000)
}

func logbackValue(l Level) int {
	switch {
	case l <= LevelTrace:
		return 5000
	case l <= LevelDebug:
		return 10000
	case l <= LevelInfo:
		return 20000
	case l <= LevelWarn:
		return 30000
	}
	return 40000
}

func otelNumber(l Level) int {
	switch {
	case l <= LevelTrace:
		return 1
	case l <= LevelDebug:
		return 5
	case l <= LevelInfo:
		return 9
	case l <= LevelWarn:
		return 13
	case l <= LevelError:
		return 17
	}
	return 21
}

// Render lays the record out as one JSON object.
func (j *JSON) Render(r *Record, _ Theme) string {
	m := meta()
	l := r.Level
	var out []pair
	add := func(k, v string) { out = append(out, pair{k, v}) }
	when := func(cond bool, k, v string) {
		if cond {
			add(k, v)
		}
	}
	line := strconv.Itoa(r.Line)
	switch j.Schema {
	case "", "serlog":
		add("time", quote(iso(r.Time, 3)))
		add("level", j.level(r, l.String(), false))
		add("target", quote(r.Target))
		add("msg", quote(r.Msg))
		when(r.File != "", "file", quote(r.File))
		when(r.Line > 0, "line", line)
		out = append(out, j.fields(r, "error")...)
		when(m.App != "", "app", quote(m.App))
		when(m.Env != "", "env", quote(m.Env))
		when(m.Instance != "", "instance", quote(m.Instance))
	case "pino":
		add("level", j.level(r, l.String(), true))
		add("time", strconv.FormatInt(r.Time.UnixMilli(), 10))
		add("pid", pid)
		add("hostname", quote(hostName()))
		out = append(out, j.fields(r, "err")...)
		add("msg", quote(r.Msg))
	case "bunyan":
		add("name", quote(m.App))
		add("hostname", quote(hostName()))
		add("pid", pid)
		add("level", j.level(r, l.String(), true))
		out = append(out, j.fields(r, "err")...)
		add("msg", quote(r.Msg))
		add("time", quote(iso(r.Time, 3)))
		add("v", "0")
	case "zap":
		add("level", j.level(r, l.String(), false))
		add("ts", seconds(r))
		add("logger", quote(r.Target))
		caller := r.File
		if r.File != "" && r.Line > 0 {
			caller += ":" + line
		}
		when(caller != "", "caller", quote(caller))
		add("msg", quote(r.Msg))
		out = append(out, j.fields(r, "error")...)
	case "slog":
		add("time", quote(strftime(r.Time.Local(), "%Y-%m-%dT%H:%M:%S%.9f%:z")))
		add("level", j.level(r, l.Label(CompatSlog), false))
		add("msg", quote(r.Msg))
		out = append(out, j.fields(r, "error")...)
	case "structlog":
		add("event", quote(r.Msg))
		out = append(out, j.fields(r, "error")...)
		add("level", j.level(r, l.String(), false))
		add("logger", quote(r.Target))
		add("timestamp", quote(iso(r.Time, 3)))
	case "logstash":
		add("@timestamp", quote(strftime(r.Time.Local(), "%Y-%m-%dT%H:%M:%S%.3f%:z")))
		add("@version", quote("1"))
		add("message", quote(r.Msg))
		add("logger_name", quote(r.Target))
		add("thread_name", quote(r.Thread))
		add("level", j.level(r, l.Label(CompatLogback), false))
		add("level_value", strconv.Itoa(logbackValue(l)))
		out = append(out, j.fields(r, "stack_trace")...)
	case "ecs":
		add("@timestamp", quote(iso(r.Time, 3)))
		add("log.level", j.level(r, l.String(), false))
		add("message", quote(r.Msg))
		add("ecs.version", quote("8.11.0"))
		add("log.logger", quote(r.Target))
		when(r.File != "", "log.origin.file.name", quote(r.File))
		when(r.Line > 0, "log.origin.file.line", line)
		add("process.pid", pid)
		add("host.hostname", quote(hostName()))
		when(m.App != "", "service.name", quote(m.App))
		when(m.Env != "", "service.environment", quote(m.Env))
		when(m.Instance != "", "service.node.name", quote(m.Instance))
		out = append(out, j.fields(r, "error")...)
	case "gelf":
		add("version", quote("1.1"))
		add("host", quote(hostName()))
		add("short_message", quote(r.Msg))
		add("timestamp", seconds(r))
		add("level", strconv.Itoa(l.syslog()))
		add("_target", quote(r.Target))
		for _, f := range r.Fields {
			when(f.Key != "id", "_"+f.Key, jsonValue(f.Value))
		}
	case "otel":
		var attrs, res []pair
		attrs = append(attrs, pair{"code.namespace", quote(r.Target)})
		if r.File != "" {
			attrs = append(attrs, pair{"code.filepath", quote(r.File)})
		}
		if r.Line > 0 {
			attrs = append(attrs, pair{"code.lineno", line})
		}
		for _, f := range r.Fields {
			k := f.Key
			if k == "error" {
				k = "exception"
			}
			attrs = append(attrs, pair{k, jsonValue(f.Value)})
		}
		for _, id := range []pair{{"service.name", m.App}, {"deployment.environment", m.Env}, {"service.instance.id", m.Instance}} {
			if id.json != "" {
				res = append(res, pair{id.key, quote(id.json)})
			}
		}
		add("timestamp", quote(iso(r.Time, 9)))
		add("severity_text", quote(l.Label(CompatSerlog)))
		add("severity_number", strconv.Itoa(otelNumber(l)))
		add("body", quote(r.Msg))
		add("attributes", j.object(attrs))
		add("resource", j.object(res))
	}
	return j.object(out)
}

// Caller is true: every schema but GELF may show where a record was made.
func (j *JSON) Caller() bool { return j.Schema != "gelf" }
