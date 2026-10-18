# serlog

A modern, high-performance logging framework: any number of sinks, each configured on its own, a drop-in match for the popular loggers through compat profiles. The same model and config format exist for Python, TypeScript, Java and Go.

```toml
[dependencies]
serlog = { version = "0.1", features = ["file", "loki", "config"] }
```

## Start

With no set-up at all, `info` and above go to standard output:

```rust
serlog::info!("synced {} files", 42);
```

`init` reads `SERLOG_CONFIG` or `serlog.{yaml,yml,json,toml}` when there is one, and honours `SERLOG_FILTER` and `RUST_LOG`:

```rust
fn main() -> serlog::Result {
    let _log = serlog::init()?;
    serlog::info!(mirror = "debian", tries = 3, "synced");
    Ok(())
}
```

Keep the returned guard: queued records are flushed when it drops.

## Several sinks, each its own way

```rust
use serlog::{Level, OnFail, Queue, SinkExt, Tiers, sinks::{Console, File, Loki, Mail, Rotate}};

let _log = serlog::builder()
    .app("mirror")
    .sink(Console::new())
    .sink(File::new("logs/app.log").rotate(Rotate::Day).keep(7).gzip().name("app"))
    .sink(File::new("logs/audit.log").filter("audit=info".parse()?).on_fail(OnFail::Panic))
    .sink(Loki::new("http://alloy:3100/loki/api/v1/push").label("team", "mirrors"))
    .sink(Mail::new("smtp.example.org").auth("bot@example.org", pass).to("ops@example.org").name("oncall").alarm_only())
    .alarm(Tiers::new([["oncall"], ["stderr"]]))
    .init()?;
```

Every sink sits in a slot with its own name, level or filter, processors, layout, queue, retries and failure policy. Names default to the kind and a number, `file-1`, `file-2`.

## Records

```rust
serlog::warn!(target: "sync", tags: ["disk"], mirror = %name, free = 0.09, ?state, "disk {}% full", 91);
let _g = serlog::bind!(request = id);
serlog::error!(error = serlog::err(&e), "gave up");
serlog::fatal!("cannot go on");
```

Fields keep their types: numbers stay numbers in JSON, maps become nested objects, errors carry their causes. Arguments are not evaluated below the level. `serlog::catch_panics()` logs every panic at `fatal`.

## When a sink fails

A failing sink goes `down`, is skipped until its next probe, and raises one incident on the alarm route, not one per record. The alarm walks its tiers until one sink takes it; a recovery is told too. `OnFail::Panic` panics in the caller, `OnFail::Ignore` only counts. `serlog::status()` shows each slot's health and counts.

## Layouts

```rust
use serlog::{Compat, Layout, layout::Schema};

serlog::builder().compat(Compat::EnvLogger);            // env_logger's pattern, stderr, `error`, RUST_LOG
File::new("app.json").layout(Layout::json(Schema::Ecs));
Console::new().layout(Layout::pattern("{time:utc} {level:<5} {target:~20}: {msg}{fields}")?);
Console::new().layout(Layout::Pretty);
```

Presets: `serlog`, `tracing`, `env_logger`, `python`, `logback`, `slog`, `pino`. JSON schemas: `serlog`, `pino`, `bunyan`, `zap`, `slog`, `structlog`, `logstash`, `ecs`, `gelf`, `otel`.

## Processors

```rust
use serlog::process::{Dedupe, Fields, Redact, Sample};

serlog::builder()
    .process(Fields::new().with("region", "cn"))
    .process(Redact::keys(["pass", "token"]).path("req.headers.authorization"))
    .sink(Console::new().process(Sample::new(100, 10, Duration::from_secs(1))).process(Dedupe::new(Duration::from_secs(5))));
```

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

`serlog::config::load(path)` starts from a file, `config::from_value` from a table inside another program's config, `config::slot` turns one sink entry into a slot for `serlog::add`, and `config::dump(Format::Yaml)` writes the settings in force back out.

## Your own sink

```rust
struct Kafka { topic: String }

impl serlog::Sink for Kafka {
    fn emit(&self, entry: &serlog::Entry<'_>) -> serlog::Result {
        send(&self.topic, entry.line()).map_err(serlog::Error::other)
    }
}

serlog::config::kinds::sink("kafka", |o| Ok(Box::new(Kafka { topic: o.need("topic")? })));
```

Queues, retries, health and alarms come from the slot, so a sink only delivers. Closures work too: `serlog::from_fn(|e| println!("{}", e.line()))`. Processors, layouts and alarms register the same way.

## From `log` and `tracing`

```rust
serlog::Bridge::install()?;                                     // the `log` crate
tracing_subscriber::registry().with(serlog::Layer).init();      // `tracing`, span chain included
```

## Features

| Feature | Adds |
|:--:|:--:|
| `console` (default) | the console sink |
| `regex` (default) | message filters and regex redaction |
| `file` | the file sink |
| `loki` | the Loki and Alloy sink |
| `mail` | the mail sink |
| `config` | YAML, JSON and TOML config |
| `log`, `tracing` | the bridges |
| `full` | all of them |

Rust 1.98 or later. MIT licence.
