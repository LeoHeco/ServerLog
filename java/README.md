# serlog

A modern, high-performance logging framework: any number of sinks, each configured on its own, a drop-in match for the popular loggers through compat profiles. The same model and config format exist for Rust, Python, TypeScript and Go.

```xml
<dependency>
  <groupId>io.github.leoheco</groupId>
  <artifactId>serlog</artifactId>
  <version>0.1.0</version>
</dependency>
```

Config files need Jackson 3 for their format (`jackson-databind`, `jackson-dataformat-yaml`, `jackson-dataformat-toml`); the mail sink needs `org.eclipse.angus:angus-mail`.

## Start

With no set-up at all, `info` and above go to standard output:

```java
import io.github.leoheco.serlog.Serlog;

Serlog.info("synced {} files", 42);
var log = Serlog.get(Sync.class).with("mirror", "debian");
log.atWarn().field("tries", 3).tag("sync").log("gave up after {} tries", 3);
log.error("rsync failed", e);                      // a trailing throwable becomes the `error` field
```

`Serlog.init()` reads `SERLOG_CONFIG` or `serlog.{yaml,yml,json,toml}` when there is one, and honours `SERLOG_FILTER`, `SERLOG_COMPAT` and `SERLOG_FORMAT`.

## Several sinks, each its own way

```java
Serlog.builder()
        .app("mirror")
        .sink(new ConsoleSink())
        .sink(new FileSink("logs/app.log").rotate(FileSink.Rotate.DAY).keep(7).gzip(true).name("app"))
        .sink(new FileSink("logs/audit.log").filter("audit=info").onFail("panic"))
        .sink(new LokiSink("http://alloy:3100/loki/api/v1/push").labels(Map.of("team", "mirrors")))
        .sink(new MailSink("smtp.example.org").login("bot@example.org", pass).to("ops@example.org").name("oncall").alarmOnly(true))
        .alarm("default", Tiers.of("oncall", "stderr"))
        .init();
```

Every sink takes its slot settings alongside its own: `name`, `level` or `filter`, `process`, `format`, `pattern`, `schema`, `colour`, `alarmOnly`, `onFail`, `queue`, `retries`, `probe`. Any sink, a lambda taking an entry included, goes into a slot with `Slot.of(sink).name("audit").queue(1000)`. `Serlog.init(tree)` and `Slot.of(sink).with(entry)` take config maps as they are. Names default to the kind and a number, `file-1`, `file-2`.

## Records

Fields keep their types: numbers stay numbers in JSON, maps nest, throwables carry their causes and stack. A `Supplier` argument is called only when the record passes; `Serlog.bind(Map.of("request", 7), () -> ...)` adds fields to everything logged inside, through a scoped value. `Serlog.catchUncaught()` logs uncaught exceptions at `fatal`.

With serlog on the class path, SLF4J 2 writes through it: MDC entries and key-value pairs become fields, markers become tags. JUL and the Log4j API come in through `jul-to-slf4j` and `log4j-to-slf4j`.

## When a sink fails

A failing sink goes `down`, is skipped until its next probe, and raises one incident on the alarm route, not one per record. The alarm walks its tiers until one sink takes it; recovery is told too. `onFail("panic")` throws in the caller, `"ignore"` only counts. `Serlog.status()` shows each slot's health and counts. Queues drain on virtual threads, and calls into one sink are serialised.

## Layouts, profiles and processors

```java
Serlog.builder().compat(Compat.LOGBACK).init();
new FileSink("app.json").format("json").schema(JsonLayout.Schema.ECS);
new ConsoleSink().pattern("{time:utc} {level:<5} {target:~20}: {msg}{fields}");
new ConsoleSink().format("pretty");

Serlog.builder().process(new Fields(Map.of("region", "cn"))).process(Redact.keys("pass").paths("req.headers.authorization")).init();
new ConsoleSink().process(new Sample(100, 10, Duration.ofSeconds(1)), new Dedupe(Duration.ofSeconds(5)));
```

Profiles: `serlog`, `tracing`, `env_logger`, `python`, `logback`, `slog`, `pino`. JSON schemas: `serlog`, `pino`, `bunyan`, `zap`, `slog`, `structlog`, `logstash`, `ecs`, `gelf`, `otel`. Any lambda taking a record and returning it, a changed copy or `null` is a processor.

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

`Config.load(path)` starts from a file and reloads it when `watch` is set; `Config.dump(Format.YAML)` writes the settings in force back out.

## Your own sink

```java
public final class KafkaSink extends SinkBase<KafkaSink> {
    private final String topic;

    public KafkaSink(String topic) {
        this.topic = topic;
    }

    @Override
    public String kind() {
        return "kafka";
    }

    @Override
    public void emit(Entry entry) throws Exception {
        send(topic, entry.line());
    }
}

Kinds.sink("kafka", o -> new KafkaSink(o.string("topic")));
```

Queues, retries, health and alarms come from the slot, so a sink only delivers. `Kinds.processor`, `Kinds.layout` and `Kinds.alarm` register the other kinds.

Java 25 or later. MIT licence.
