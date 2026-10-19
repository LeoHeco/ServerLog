<p align="center"><img src="site/static/img/logo.svg" width="96" alt=""></p>

# serlog

A modern, high-performance logging framework: any number of sinks, each configured on its own, a drop-in match for the popular loggers of each language through compat profiles.

Documentation: https://leoheco.github.io/ServerLog/

| Language | Package | Directory |
|:--:|:--:|:--:|
| Rust | crates.io `serlog` | `rust/` |
| Python | PyPI `serlog` | `python/` |
| TypeScript | npm `serlog` | `node/` |
| Java | Maven Central `io.github.leoheco:serlog` | `java/` |
| Go | `github.com/LeoHeco/ServerLog/go` | `go/` |

Every package has the same model: loggers make records; a hub filters and processes them and hands them to slots; each slot wraps one sink with its own filter, processors, layout, queue, retries, health and failure policy. A failing sink raises one incident on a separate alarm route, which walks tiers of other sinks until one takes it.

Built in: console, file with rotation and clean-up, Grafana Loki (and Alloy), mail, and an in-memory ring for tests. Layouts: patterns, logfmt, pretty, and JSON in ten schemas from pino to OpenTelemetry. New sinks, processors, layouts and alarms plug in from code and config files alike.

`spec/` holds what every package shares: the specification, the config JSON Schema, the test vectors every package passes, and the mail template.

## Licence

MIT
