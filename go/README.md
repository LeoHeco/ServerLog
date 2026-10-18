# serlog

A modern, high-performance logging framework: any number of sinks, each configured on its own, a drop-in match for the popular loggers through compat profiles. It is a `log/slog` handler, and the same model and config format exist for Rust, Python, TypeScript and Java.

```sh
go get github.com/LeoHeco/ServerLog/go
```

## Start

With no set-up at all, `info` and above go to standard output:

```go
import serlog "github.com/LeoHeco/ServerLog/go"

serlog.Info("synced files", "n", 42, "mirror", "debian")
log := serlog.Get("app.sync", "mirror", "debian")   // a *slog.Logger
log.Warn("gave up", "tries", 3, serlog.Tags("sync"), serlog.Err(err))
defer serlog.Flush()
```

`serlog.Init(serlog.Config{})` reads `SERLOG_CONFIG` or `serlog.{yaml,yml,json,toml}` when there is one, and honours `SERLOG_FILTER`, `SERLOG_COMPAT`, `SERLOG_FORMAT` and `DEBUG`. `serlog.Capture()` makes serlog slog's default handler, so `slog.Info` and the `log` package write through it.

## Several sinks, each its own way

```go
err := serlog.Init(serlog.Config{
	App: "mirror",
	Sinks: []serlog.Sink{
		&serlog.Console{},
		&serlog.File{Slot: serlog.Slot{Name: "app"}, Path: "logs/app.log", Rotate: "day", Keep: 7, Gzip: true},
		&serlog.File{Slot: serlog.Slot{Filter: "audit=info", OnFail: "panic"}, Path: "logs/audit.log"},
		&serlog.Loki{URL: "http://alloy:3100/loki/api/v1/push", Labels: map[string]string{"team": "mirrors"}},
		&serlog.Mail{Slot: serlog.Slot{Name: "oncall", AlarmOnly: true}, Host: "smtp.example.org", User: "bot@example.org", Pass: pass, To: []string{"ops@example.org"}},
	},
	Alarms: map[string]serlog.Alarm{"default": &serlog.Tiers{Tiers: [][]string{{"oncall"}, {"stderr"}}}},
})
```

Every built-in sink embeds `serlog.Slot`: `Name`, `Level` or `Filter`, `Process`, `Format`, `Pattern`, `Schema`, `Colour`, `AlarmOnly`, `OnFail`, `Queue`, `Retries`, `Probe`. `serlog.With(sink, serlog.Slot{...})` puts any sink, a `serlog.SinkFunc` included, in a slot. `serlog.Parse(tree)` turns a config map into a `Config`. Names default to the kind and a number, `file-1`, `file-2`.

## Records

Attributes keep their types: numbers stay numbers in JSON, groups nest, errors carry what they wrap. `slog.LogValuer` values are worked out only when the record passes. `serlog.Bind(ctx, "request", 7)` adds fields to everything logged with that context.

## When a sink fails

A failing sink goes `down`, is skipped until its next probe, and raises one incident on the alarm route, not one per record. The alarm walks its tiers until one sink takes it; recovery is told too. `OnFail: "panic"` panics in the caller, `"ignore"` only counts. `serlog.Statuses()` shows each slot's health and counts. Queues drain on goroutines, and calls into one sink are serialised. Inside a sink, log with `entry.Context()` so what it logs is dropped instead of looping back.

## Layouts, profiles and processors

```go
serlog.Init(serlog.Config{Compat: serlog.CompatSlog})
&serlog.File{Slot: serlog.Slot{Format: "json", Schema: "ecs"}, Path: "app.json"}
&serlog.Console{Slot: serlog.Slot{Pattern: "{time:utc} {level:<5} {target:~20}: {msg}{fields}"}}
&serlog.Console{Slot: serlog.Slot{Format: "pretty"}}

serlog.Config{Process: []serlog.Processor{serlog.Fields{"region": "cn"}, &serlog.Redact{Keys: []string{"pass"}, Paths: []string{"req.headers.authorization"}}}}
serlog.Slot{Process: []serlog.Processor{&serlog.Sample{First: 100, Every: 10, Per: time.Second}, &serlog.Dedupe{Within: 5 * time.Second}}}
```

Profiles: `serlog`, `tracing`, `env_logger`, `python`, `logback`, `slog`, `pino`. JSON schemas: `serlog`, `pino`, `bunyan`, `zap`, `slog`, `structlog`, `logstash`, `ecs`, `gelf`, `otel`. A `serlog.ProcessorFunc` returning the record, a changed copy or `nil` is a processor.

## Config files

```yaml
app: mirror
filter: info,hustsync=debug
redact: [pass]
sinks:
  - kind: console
  - { kind: file, name: app, path: /var/log/app.log, max_size: 64MiB, keep: 7, gzip: true }
  - { kind: loki, url: "http://alloy:3100/loki/api/v1/push", on_fail: ops }
alarms:
  ops: { tiers: [[stderr]], remind: 30m }
watch: 30s
```

`serlog.Load(path)` starts from a file and reloads it when `watch` is set; `serlog.Dump(serlog.FormatYAML)` writes the settings in force back out.

## Your own sink

```go
type Kafka struct {
	serlog.Slot
	Topic string
}

func (k *Kafka) Kind() string                   { return "kafka" }
func (k *Kafka) Emit(e *serlog.Entry) error     { return send(k.Topic, e.Line()) }

serlog.RegisterSink("kafka", func(o *serlog.Options) (serlog.Sink, error) {
	return &Kafka{Topic: o.Need("topic")}, nil
})
```

Queues, retries, health and alarms come from the slot, so a sink only delivers. `RegisterProcessor`, `RegisterLayout` and `RegisterAlarm` register the other kinds.

Go 1.27 or later. MIT licence.
