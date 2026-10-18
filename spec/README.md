# Shared specification

Every serlog package follows this file and passes the vectors in `cases/`. A change here lands in all packages in the same commit. Packages differ only in how each language spells things; names follow each language's own conventions, so `max_size` here is `maxSize` in TypeScript and Java.

## Model

| Part | Does | Does not |
|:--:|:--:|:--:|
| Logger | makes records with a target and bound fields; skips argument evaluation below the level | decide where records go |
| Record | level, target, message, typed fields, tags, caller, thread, span chain, time, sequence number | know how it is shown |
| Filter | admits or rejects by target directives and a message regex; knows the lowest level it admits | change records |
| Processor | adds fields, changes the message, or drops the record | write anything |
| Layout | turns a record into text | know where the text goes |
| Sink | delivers entries somewhere and reports what went wrong | queue, retry, alarm or lay out |
| Slot | wraps one sink: name, filter, processors, layout, queue, retries, health, failure policy | know how the sink writes |
| Hub | global filter, scope context, global processors, the slot table, dispatch | anything about one sink |
| Alarm | chooses the tiers for an incident and the record sent | touch health or bypass exclusion |
| Alarm task | consumes incidents, walks tiers, collects failures, applies the alarm's fallback | choose routes |

A sink receives an entry: the record plus the line its slot's layout makes of it, rendered on first use. One record going to several slots with the same layout and colouring is rendered once.

Two routes, both producer and consumer:

| Route | Producer | Queue | Consumer |
|:--:|:--:|:--:|:--:|
| log | the calling code | one per queued slot | one background task per queued slot, writing in batches |
| alarm | a failing slot | one, process-wide | one alarm task, one incident at a time |

Queues, their tasks, the default alarm and the alarm task are created on first use.

## Defaults

- Without any set-up, records at `info` and above go to a console sink on standard output.
- `init()` with no arguments reads the file named by `SERLOG_CONFIG`, or the first of `serlog.yaml`, `serlog.yml`, `serlog.json` and `serlog.toml` in the working directory. Failing that it starts a console sink.
- `init()` given no sinks starts a console sink; to log nothing, set the level to `off`.
- The default alarm routes to `stderr`, a reserved console sink on standard error that is not on the log route. A sink named `stderr` replaces it.
- Environment variables: `SERLOG_FILTER` overrides the filter (Rust also reads `RUST_LOG`); `SERLOG_COMPAT` overrides the compat profile; `SERLOG_FORMAT` sets the zero-config console's format; `NO_COLOR` and `FORCE_COLOR` govern colour.

| Setting | Default |
|:--:|:--:|
| name | kind and number: `file-1`, `file-2` |
| on_fail | the `default` alarm |
| `default` alarm | `tiers: [[stderr]]`, `on_fail: ignore`, `remind: 1h` |
| queue | console, file and memory write directly; loki 8192 records, 1000 a batch or 1 s; mail 1024, 1000 a batch or 60 s; `full: drop` |
| retries | 0 when direct; 3 behind a queue, pausing 1, 2, 4 s |
| probe | 5 s, doubling on each failed probe, at most 5 min |
| network timeout | 10 s |
| flush at exit | at most 5 s |

## Levels

| Level | Value | Colour |
|:--:|:--:|:--:|
| `trace` | 10 | cyan |
| `debug` | 20 | magenta |
| `info` | 30 | green |
| `warn` | 40 | yellow |
| `error` | 50 | red |
| `fatal` | 60 | white on red |

Names are case-insensitive; `warning` means `warn`, `critical` means `fatal`, and a number from 0 to 255 is a level by value. Levels compare by value only. Custom levels have a lower-case name, a value and a colour, and work in filters, calls and layouts alike.

How levels are shown depends on the profile; a pattern preset uses its own logger's names, other patterns the profile in force:

| Profile | trace | debug | info | warn | error | fatal |
|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| `python` | TRACE | DEBUG | INFO | WARNING | ERROR | CRITICAL |
| `slog` | DEBUG-4 | DEBUG | INFO | WARN | ERROR | ERROR+4 |
| any other | TRACE | DEBUG | INFO | WARN | ERROR | FATAL |

A custom level shows as its name in upper case.

## Records and values

Field values are typed: null, boolean, integer, float, string, list, map, error, or anything else shown as a string. Fields come in order: scope, then logger, then the call; a later field of the same name wins. `error` holds an error and `tags` lists tags.

| Type | Text | JSON |
|:--:|:--:|:--:|
| null, boolean, number | as is; `null`, `true` | the JSON type |
| string | as is; quoted the logfmt way under `q` | string |
| list | `[a, b]` | array |
| map | spread as `key.sub=value` | object |
| error | message and causes: `connect: refused` | `{"message": …, "causes": [ … ]}` |

`q` writes a value bare unless it is empty or holds whitespace, `"` or `=`; then it writes a JSON string.

## Filters

Comma-separated directives, as in `RUST_LOG`:

| Directive | Meaning |
|:--:|:--:|
| `info` | the default level |
| `app=debug` | target `app` and everything under it |
| `app` | everything for `app` |
| `app=off` | nothing for `app` |
| `web:*=error` | `*` matches any characters |
| `info/sync` | everything after the first `/` is a regex the message must match |

- A target is matched with the parts under it, split at `::`, `.`, `:` or `/`.
- The most specific directive wins: literal targets by length, then wildcard ones by literal length.
- Targets no directive matches are off unless there is a default level; an empty spec lets `error` and above through.

## Processors

A chain runs on every record in the hub, then each slot may run its own before laying out. A step may change the record or drop it. Steps that only decide do not copy the record; the first step to change it copies it, so other slots still see the original.

| Step | Options | Effect |
|:--:|:--:|:--:|
| `redact` | `keys`, `paths`, `patterns`, `mask` | masks fields with these names at any depth, and fields at these dotted paths, with `mask`, `***` by default; replaces regex matches in the message |
| `sanitise` | none | escapes line breaks, tabs and control characters in the message and string fields as `\n`, `\r`, `\t`, `\u{0007}` |
| `fields` | key-value pairs | sets these fields on every record |
| `match` | `tags`, `fields`, `level` | keeps the record only when every condition holds |
| `exclude` | the same | drops the record when every condition holds |
| `sample` | `first`, `every`, `per` | per level and message, keeps the first `first` in each `per`, then one in `every` |
| `limit` | `rate`, `burst` | a token bucket of `rate` a second holding up to `burst` |
| `dedupe` | `within` | drops a record with the level, target and message of one kept within `within`; the next one kept carries `repeated`, the number dropped |

Top-level `redact: [pass, token]` is short for a `redact` step with those keys.

## Layouts

| `format` | Output |
|:--:|:--:|
| `text` | a pattern; the profile's pattern unless `pattern` is given |
| `logfmt` | `time=2023-11-14T22:13:20Z level=warn target=app msg="…" key=value` |
| `json` | an object in a schema |
| `pretty` | time, level, target and message, then one aligned `key = value` line per field |

### Patterns

Literal text with `{token}` or `{token:options}` holes; `{{` and `}}` are literal braces. Every token but `time` may end with `<N`, `>N` or `^N` to pad to N columns, then `.M` to keep at most M characters. Centring puts the extra space on the right.

| Token | Options | Output |
|:--:|:--:|:--:|
| `time` | `local` (default), `utc`, `rfc3339`, `unix`, `unix_ms`, `unix_ns`, or strftime in local time | `2023-11-14 23:13:20.000`, `2023-11-14T22:13:20Z`, `2023-11-14T23:13:20.000000+01:00`, `1700000000` |
| `level` | `lower` | `WARN`, `warn` |
| `target` | `~N` shortens leading segments to their first letter until it fits N | `a::sync` |
| `module`, `func` | | caller; `?` when unknown |
| `file` | `base` keeps the file name | `src/sync.rs`; `?` when unknown |
| `line` | | `12`; `?` when unknown |
| `thread`, `tid`, `task` | | thread name, thread id, async task name |
| `pid`, `host`, `app`, `env`, `instance` | | process and deployment identity |
| `msg`, `fields` | `q` | the message; ` key=value` for each field |
| `tags` | | comma-separated |
| `error` | `stack` | the `error` field with its causes; `{fields}` then leaves it out |
| `elapsed`, `seq` | | milliseconds since start; the record's number |
| `spans` | | the span chain and `: `, when there is one |

### Presets

A preset name may stand wherever a pattern is expected, and shows levels the way its logger does.

| Name | Pattern | Reproduces |
|:--:|:--:|:--:|
| `serlog` | `{time} {level:<5} {target}: {msg}{fields}` | the default |
| `tracing` | `{time:rfc3339} {level:>5} {spans}{target}: {msg}{fields}` | `tracing-subscriber` |
| `env_logger` | `[{time:utc} {level:<5} {target}] {msg}` | `env_logger` |
| `python` | `{level}:{target}:{msg}` | `logging.basicConfig` |
| `logback` | `{time:%H:%M:%S%.3f} [{thread}] {level:<5} {target:~36} - {msg}` | Logback, Log4j 2 |
| `slog` | `time={time:%Y-%m-%dT%H:%M:%S%.3f%:z} level={level} msg={msg:q}{fields:q}` | Go `slog.TextHandler` |
| `pino` | `[{time:%H:%M:%S%.3f}] {level} ({pid}): {msg}{fields}` | `pino-pretty` |

### JSON schemas

Keys in this order; `[fields]` is where the fields go, `error` renamed as shown.

| Schema | Keys |
|:--:|:--:|
| `serlog` | `time` UTC RFC 3339 with ms, `level` name, `target`, `msg`, `file`, `line`, [fields], `app`, `env`, `instance` when set |
| `pino` | `level` value, `time` Unix ms, `pid`, `hostname`, [fields, error as `err`], `msg` |
| `bunyan` | `name` app, `hostname`, `pid`, `level` value, [fields, error as `err`], `msg`, `time` UTC RFC 3339 with ms, `v` 0 |
| `zap` | `level` name, `ts` Unix seconds with 3 decimals, `logger` target, `caller` `file:line`, `msg`, [fields] |
| `slog` | `time` local RFC 3339 with ns, `level` slog label, `msg`, [fields] |
| `structlog` | `event`, [fields], `level` name, `logger` target, `timestamp` UTC RFC 3339 with ms |
| `logstash` | `@timestamp` local RFC 3339 with ms, `@version` `"1"`, `message`, `logger_name`, `thread_name`, `level` upper case, `level_value` 5000 to 40000, [fields, error as `stack_trace`] |
| `ecs` | `@timestamp`, `log.level`, `message`, `ecs.version` `8.11.0`, `log.logger`, `log.origin.file.name`, `log.origin.file.line`, `process.pid`, `host.hostname`, `service.name`, `service.environment`, `service.node.name` when set, [fields] |
| `gelf` | `version` `1.1`, `host`, `short_message`, `timestamp` Unix seconds with 3 decimals, `level` syslog severity, `_target`, fields with a `_` prefix, `id` left out |
| `otel` | `timestamp` UTC RFC 3339 with ns, `severity_text`, `severity_number` 1, 5, 9, 13, 17, 21, `body`, `attributes` with `code.*` and the fields, `resource` with `service.*` |

`keys` renames any key, `levels` writes levels as `name` or `number`, and `nest` puts the fields under one key.

### Colour

`colour` is `auto`, `always` or `never`. `auto` colours when the sink is a terminal, never when `NO_COLOR` is set, always when `FORCE_COLOR` is set. `colours` gives levels other colours: `plain`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, `grey`, `alarm`. Patterns colour the level; `pretty` also greys the time and colours field names.

## Sinks

A sink offers: write one entry; write a batch, one by one unless it does better; whether it wants a level and target; flush; close; a start-up check; a resource it claims; its kind; whether it is a terminal; its default layout; its default queue; and its options for export. Only writing one entry is required.

| Kind | Check at registration | Claims | A batch | Default queue | Default layout |
|:--:|:--:|:--:|:--:|:--:|:--:|
| `console` | none | nothing | one write per stream | direct | profile |
| `file` | makes the directory, opens with `append` or truncates, sets `perms` on a new file; fails on a directory | its canonical absolute path | one write | direct | profile |
| `loki` | URL is http or https; with `connect`, `GET /ready` | nothing | one push, a stream per level | 8192, 1000, 1 s | `{msg}` |
| `mail` | recipients, a sender (`from` or `user`), a password for a user; with `connect`, logs in | nothing | one message | 1024, 1000, 60 s | profile, for the plain text |
| `memory` | none | nothing | appended | direct | profile |

- **console**: `stream` is `stdout` (default), `stderr` or `auto`, which sends `error` and above to standard error.
- **file**: `path`; `append` (default true); `perms` such as `"0640"`; `mkdir` (default true); `max_size`; `rotate`: `minute`, `hour`, `day` or `week` on local time; `keep`, `max_age`, `max_total` for clean-up, oldest first; `gzip` in the background; `roll_name`, default `{stem}.{time:%Y%m%d-%H%M%S}.{ext}`, with `-1`, `-2` added before the extension when taken; `reopen` checks once a second whether the file was moved, truncated or deleted; `buffer` with `flush_every`, flushing at once for `error` and above.
- **loki**: `url`; `labels`; `timeout`; `connect`. Stream labels are `app`, `env`, `instance`, `level` and `labels`; `target`, `file` and the fields travel as structured metadata. Network errors, 429 and 5xx are worth retrying; other 4xx refuse the batch.
- **mail**: `host`, `port`, `security` (`tls`, `starttls` or `none`), `user`, `pass`, `from`, `to`, `timeout`, `connect`. Only `error` and above unless the slot sets a level or filter. The HTML comes from `mail.html`.
- **memory**: `size`, the newest records kept; read back records and lines, find, clear.

## Slots

A slot's settings: `name`, `level` or `filter`, `process`, the layout keys, `colour`, `colours`, `alarm_only`, `on_fail`, `queue` (`0` for direct), `batch`, `wait`, `full` (`drop` or `block`), `retries`, `probe`.

Registration names the slot, refuses a name already taken or `*`, refuses a resource another slot claims, then runs the sink's check. Config and builders register all or nothing. Removing a slot, or dropping it when a reloaded config replaces the slots, writes what its queue holds, stops its task and closes its sink.

A record goes through the slot's filter, the sink's level check and the slot's processors. A queued slot copies it into its queue, dropping it when full or, with `full: block`, waiting while the sink is up. A direct slot skips it while down and not due for a probe, counting it as skipped; otherwise it writes, retrying without pause.

The queue task gathers up to `batch` records or waits `wait` after the first, then writes them, pausing between retries. A failed batch is kept and written again at the next probe; a refused one is dropped and counted.

## Failure handling

| Policy | Direct failure | Background failure | Health |
|:--:|:--:|:--:|:--:|
| `panic` | raised in the caller | one line on standard error, one second for other sinks to flush, exit code 70 | not tracked; every failure raises |
| `ignore` | counted | counted | tracked, nothing told |
| an alarm | an incident on the alarm route | the same | tracked, told once per failure |

Health is `up`, `down` or `probing`; `cases/fail.json` pins the machine:

- `up` fails: `down`, an incident `opened` when told, next probe after `probe`.
- `down` before its probe: skipped.
- `down` at its probe: `probing`; one write decides.
- `probing` succeeds: `up`, `resolved` when the failure was told, the probe interval back to its start.
- `probing` fails: `down`, the interval doubled up to 5 minutes, `reminder` when the alarm's `remind` has passed since it last told.
- A failure folded into another incident is not told, but its recovery is.

The alarm walk, pinned by `cases/alarm.json`:

1. Leave out the failing sink.
2. For each tier: `*` stands for every sink on the log route; leave out sinks already tried and those resting; deliver to the rest at once, bypassing queues; add each failure to the incident and leave that sink out from then on. One success ends the walk.
3. When no tier took it, apply the alarm's `on_fail`: `ignore` drops it, `panic` exits as above.

An incident record: `error` for `opened` and `reminder`, `info` for `resolved`; target `serlog`; message `sink `name` failed: …`, `sink `name` is still failing: …` or `sink `name` works again after Ns`; fields `sink`, `kind`, `phase`, `failures`, `skipped`, `dropped`, `since`. Alarm records skip slot filters.

Two rules stop loops: a sink that fails while an alarm is delivered rides in that incident instead of raising its own, and records made while a sink or processor runs are dropped and counted as suppressed.

## Compat profiles

`compat` changes the default pattern, the console's stream, the default level and the filter variables together.

| Profile | Stream | Level | Variables |
|:--:|:--:|:--:|:--:|
| `serlog` | stdout | info | `SERLOG_FILTER`, and `RUST_LOG` in Rust |
| `tracing` | stdout | info | `RUST_LOG` |
| `env_logger` | stderr | error | `RUST_LOG` |
| `python` | stderr | warn | none |
| `logback` | stdout | debug | none |
| `slog` | stdout | info | none |
| `pino` | stdout | info | none |

## Config

YAML, JSON or TOML, validated by `serlog.schema.json`. Unknown keys are errors naming the key. `${NAME}` in any string is replaced by the environment variable `NAME`; an unset one is an error. Durations are seconds or carry `ms`, `s`, `m`, `h` or `d`; sizes are bytes or carry `KB`, `MB`, `GB`, `KiB`, `MiB` or `GiB`.

```yaml
app: mirror
env: prod
compat: serlog
filter: info,hustsync=debug
levels: { notice: { value: 35, colour: blue } }
process: [{ fields: { region: cn } }]
redact: [pass, token]
watch: 30s
sinks:
  - kind: console
  - { kind: file, name: app, path: /var/log/app.log, max_size: 64MiB, keep: 7, gzip: true }
  - { kind: loki, url: "http://alloy:3100/loki/api/v1/push", on_fail: ops }
  - { kind: mail, name: oncall, alarm_only: true, host: smtp.example.org, user: bot@example.org, pass: "${SMTP_PASS}", to: [ops@example.org] }
alarms:
  ops: { tiers: [[oncall], [stderr]], on_fail: panic, remind: 30m }
```

| Key | Value |
|:--:|:--:|
| `app`, `env`, `instance` | identity; the program name, empty and the host name by default |
| `compat` | a profile |
| `level`, `filter`, `filter_env` | the default level, directives, a variable that overrides them |
| `levels` | custom levels: a value, or `value` and `colour` |
| `process`, `redact` | the global chain; masking shorthand |
| `sinks` | the sinks, each with `kind`, slot settings and its own keys |
| `alarms` | named alarms; `default` replaces the default; `panic` and `ignore` cannot be names; each takes `kind` (default `tiers`), `tiers`, `on_fail` (`panic` or `ignore`), `remind` |
| `watch` | how often to check the file for changes |

A processor in a list is its name, or a map from its name to its options. A sink's `on_fail` is `panic`, `ignore`, `default` or a name in `alarms`.

- **load**: from a file, the format from its extension, or from text.
- **dump**: the settings in force as YAML, JSON or TOML; loading the result behaves the same, except that passwords come out as `***`. A custom sink, processor, layout or alarm without options cannot be dumped, and the error names it.
- **watch**: when the file's modification time changes, the whole config is loaded and checked again; it replaces the old one only when all of it passes. A failure goes to `stderr` and the old config stays.
- Code and config share keys: every key has a setter of the same name, and a config entry can be passed to code as a dictionary or options object.

## Extending

Four kinds of extension, each usable from code directly or registered by name for config files: sinks (`kind`), processors (a `process` entry), layouts (`layout: name`) and alarms (`kind` under `alarms`). A registered factory receives the entry's own keys and must leave none unread.

## Status

Each slot reports its name, kind, health, records skipped while down, records dropped by its queue, records suppressed, when its current health began, and its latest error while not up. The number of incidents dropped by a full alarm queue is available too.

## Vectors

- `cases/render.json`: patterns, presets, logfmt, pretty, JSON and its schemas, and filters, rendered with the identity in `meta`.
- `cases/fail.json`: the health machine.
- `cases/alarm.json`: the alarm walk.
