# serlog

A modern, high-performance logging framework: any number of sinks, each configured on its own, a drop-in match for the popular loggers through compat profiles. The same model and config format exist for Rust, Python, Java and Go.

```sh
npm install serlog          # YAML or TOML config: npm install yaml, or smol-toml; mail: npm install nodemailer
```

## Start

With no set-up at all, `info` and above go to standard output:

```ts
import * as serlog from "serlog";

serlog.info({ mirror: "debian" }, "synced %d files", 42);
serlog.warn("disk %s full", "91%");
```

`await serlog.init()` reads `SERLOG_CONFIG` or `serlog.{yaml,yml,json,toml}` when there is one, and honours `SERLOG_FILTER`, `SERLOG_COMPAT`, `SERLOG_FORMAT` and the `DEBUG` namespaces of the `debug` package.

## Several sinks, each its own way

```ts
await serlog.init({
  app: "mirror",
  sinks: [
    new serlog.Console(),
    new serlog.File({ path: "logs/app.log", rotate: "day", keep: 7, gzip: true, name: "app" }),
    new serlog.File({ path: "logs/audit.log", filter: "audit=info", onFail: "panic" }),
    new serlog.Loki({ url: "http://alloy:3100/loki/api/v1/push", labels: { team: "mirrors" } }),
    new serlog.Mail({ host: "smtp.example.org", user: "bot@example.org", pass, to: "ops@example.org", name: "oncall", alarmOnly: true }),
  ],
  alarms: { default: new serlog.Tiers({ tiers: [["oncall"], ["stderr"]] }) },
});
```

Every sink takes its slot settings alongside its own: `name`, `level` or `filter`, `process`, `format`, `pattern`, `schema`, `colour`, `alarmOnly`, `onFail`, `queue`, `retries`, `probe`. They are the keys of a config entry in camel case; `init` takes a parsed config tree as it is. Names default to the kind and a number, `file-1`, `file-2`.

## Records

```ts
const log = serlog.get("app.sync", { mirror: "debian" });
log.error({ tags: ["sync"], tries: 3 }, "gave up");
await serlog.bind({ request: 7 }, async () => log.info("handled"));   // across awaits and callbacks
log.exception(err, "parse failed");

const job = serlog.catching(async () => { ... }, { level: "warn", reraise: false });
```

Fields keep their types: numbers stay numbers in JSON, objects nest, errors carry their causes and stack. `serlog.lazy(fn)` defers work until a record passes. `using _ = serlog.capture()` routes `console` in; `serlog.catchUncaught()` logs uncaught errors at `fatal`.

## When a sink fails

A failing sink goes `down`, is skipped until its next probe, and raises one incident on the alarm route, not one per record. The alarm walks its tiers until one sink takes it; recovery is told too. `onFail: "panic"` throws in the caller, `"ignore"` only counts. `serlog.status()` shows each slot's health and counts.

## Layouts, profiles and processors

```ts
await serlog.init({ compat: "pino" });
new serlog.File({ path: "app.json", format: "json", schema: "ecs" });
new serlog.Console({ pattern: "{time:utc} {level:<5} {target:~20}: {msg}{fields}" });
new serlog.Console({ format: "pretty" });

await serlog.init({ process: [new serlog.Fields({ region: "cn" }), new serlog.Redact({ keys: ["pass"], paths: ["req.headers.authorization"] })] });
new serlog.Console({ process: [new serlog.Sample({ first: 100, every: 10, per: "1s" }), new serlog.Dedupe({ within: "5s" })] });
```

Profiles: `serlog`, `tracing`, `env_logger`, `python`, `logback`, `slog`, `pino`. JSON schemas: `serlog`, `pino`, `bunyan`, `zap`, `slog`, `structlog`, `logstash`, `ecs`, `gelf`, `otel`. Any function taking a record and returning it, a changed copy or `null` is a processor.

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

`await serlog.load(path)` starts from a file and reloads it when `watch` is set; `await serlog.dump("yaml")` writes the settings in force back out.

## Your own sink

```ts
class Kafka implements serlog.Sink {
  readonly kind = "kafka";
  readonly slot: serlog.SlotOptions;
  readonly #topic: string;

  constructor({ topic, ...slot }: { topic: string } & serlog.SlotOptions) {
    this.#topic = topic;
    this.slot = slot;
  }

  async emit(entry: serlog.Entry): Promise<void> {
    await send(this.#topic, entry.line);
  }
}

serlog.kind("kafka", (o) => new Kafka(o as { topic: string }));
```

Queues, retries, health and alarms come from the slot, so a sink only delivers. A plain function taking an entry is a sink too. `serlog.processor`, `serlog.layout` and `serlog.alarm` register the other kinds.

Node 26 or later, ESM and CommonJS. MIT licence.
