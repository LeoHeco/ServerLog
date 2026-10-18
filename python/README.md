# serlog

A modern, high-performance logging framework: any number of sinks, each configured on its own, a drop-in match for the popular loggers through compat profiles. The same model and config format exist for Rust, TypeScript, Java and Go.

```sh
pip install serlog          # YAML config: pip install "serlog[yaml]"
```

## Start

With no set-up at all, `info` and above go to standard output:

```python
import serlog

serlog.info("synced {} files", 42, mirror="debian")
serlog.warn(t"disk {used:.0%} full")          # t-strings: interpolations become fields
```

`serlog.init()` reads `SERLOG_CONFIG` or `serlog.{yaml,yml,json,toml}` when there is one, and honours `SERLOG_FILTER`, `SERLOG_COMPAT` and `SERLOG_FORMAT`.

## Several sinks, each its own way

```python
serlog.init(
    app="mirror",
    sinks=[
        serlog.Console(),
        serlog.File("logs/app.log", rotate="day", keep=7, gzip=True, name="app"),
        serlog.File("logs/audit.log", filter="audit=info", on_fail="panic"),
        serlog.Loki("http://alloy:3100/loki/api/v1/push", labels={"team": "mirrors"}),
        serlog.Mail("smtp.example.org", user="bot@example.org", password=pw, to="ops@example.org", name="oncall", alarm_only=True),
    ],
    alarms={"default": serlog.Tiers([["oncall"], ["stderr"]])},
)
```

Every sink takes its slot settings alongside its own: `name`, `level` or `filter`, `process`, `format`, `pattern`, `schema`, `colour`, `alarm_only`, `on_fail`, `queue`, `retries`, `probe`. They are the keys of a config entry, so `serlog.File(**entry)` and `serlog.init(**tree)` work as they are. Names default to the kind and a number, `file-1`, `file-2`.

## Records

```python
log = serlog.get("app.sync", mirror="debian")
log.error("gave up after {} tries", 3, tags=["sync"])
with serlog.bind(request=7):                   # across threads and asyncio tasks
    log.info("handled")
log.exception("parse failed")                  # inside `except`

@serlog.catch                                   # also @serlog.catch(level="warn", reraise=False), with serlog.catch():
def job(): ...
```

Fields keep their types: numbers stay numbers in JSON, dicts become nested objects, exceptions carry their causes and traceback. `serlog.lazy(fn)` defers work until a record passes. `serlog.capture()` routes the standard `logging` module and `warnings` in; `serlog.catch_uncaught()` logs uncaught exceptions at `fatal`.

## When a sink fails

A failing sink goes `down`, is skipped until its next probe, and raises one incident on the alarm route, not one per record. The alarm walks its tiers until one sink takes it; recovery is told too. `on_fail="panic"` raises in the caller, `"ignore"` only counts. `serlog.status()` shows each slot's health and counts.

## Layouts, profiles and processors

```python
serlog.init(compat="python")                    # WARNING:app:msg on standard error, warn and above
serlog.File("app.json", format="json", schema="ecs")
serlog.Console(pattern="{time:utc} {level:<5} {target:~20}: {msg}{fields}")
serlog.Console(format="pretty")

serlog.init(process=[serlog.Fields(region="cn"), serlog.Redact(["pass"], paths=["req.headers.authorization"])])
serlog.Console(process=[serlog.Sample(100, 10, "1s"), serlog.Dedupe("5s")])
```

Profiles: `serlog`, `tracing`, `env_logger`, `python`, `logback`, `slog`, `pino`. JSON schemas: `serlog`, `pino`, `bunyan`, `zap`, `slog`, `structlog`, `logstash`, `ecs`, `gelf`, `otel`. Any function taking a record and returning it, a changed copy or `None` is a processor.

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

`serlog.load(path)` starts from a file and reloads it when `watch` is set; `serlog.dump("yaml")` writes the settings in force back out.

## Your own sink

```python
@serlog.kind("kafka")
class Kafka(serlog.Sink):
    def __init__(self, topic: str, **slot):
        super().__init__(**slot)
        self.topic = topic

    def emit(self, entry: serlog.Entry) -> None:
        send(self.topic, entry.line)
```

Queues, retries, health and alarms come from the slot, so a sink only delivers. A plain function taking an entry is a sink too. `@serlog.processor`, `@serlog.layout` and `@serlog.alarm` register the other kinds.

Python 3.15 or later. MIT licence.
