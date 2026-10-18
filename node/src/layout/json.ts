/**
 * JSON lines in the shape other tools expect: serlog's own, pino, bunyan, zap, slog, structlog, logstash, ECS, GELF
 * or the OpenTelemetry log model; any key can be renamed.
 */

import { levelLabel, levelName, syslog } from "../level.ts";
import { hostName, meta } from "../meta.ts";
import type { LogRecord } from "../record.ts";
import { iso, strftime } from "./clock.ts";
import { jsonValue, quote } from "./text.ts";

/** A JSON shape. */
export type Schema = "serlog" | "pino" | "bunyan" | "zap" | "slog" | "structlog" | "logstash" | "ecs" | "gelf" | "otel";

/** Every schema. */
export const SCHEMAS: readonly Schema[] = [
  "serlog",
  "pino",
  "bunyan",
  "zap",
  "slog",
  "structlog",
  "logstash",
  "ecs",
  "gelf",
  "otel",
];

/** How levels are written. */
export type Levels = "name" | "number";

/** Options of a JSON layout. */
export interface JsonOptions {
  readonly keys?: Readonly<Record<string, string>>;
  readonly levels?: Levels;
  readonly nest?: string;
}

type Pairs = (readonly [string, string])[];

const logbackValue = (l: number): number =>
  l <= 10 ? 5000 : l <= 20 ? 10000 : l <= 30 ? 20000 : l <= 40 ? 30000 : 40000;
const otelNumber = (l: number): number => (l <= 10 ? 1 : l <= 20 ? 5 : l <= 30 ? 9 : l <= 40 ? 13 : l <= 50 ? 17 : 21);
const seconds = (t: Date): string => (t.getTime() / 1000).toFixed(3);

/** A JSON layout: a schema, renamed keys, level style and where fields go. */
export class Json {
  readonly schema: Schema;
  readonly options: JsonOptions;

  constructor(schema: Schema = "serlog", options: JsonOptions = {}) {
    if (!SCHEMAS.includes(schema)) throw new TypeError(`unknown JSON schema \`${schema}\``);
    this.schema = schema;
    this.options = options;
  }

  #key(k: string): string {
    return this.options.keys?.[k] ?? k;
  }

  #object(pairs: Pairs): string {
    return `{${pairs.map(([k, v]) => `${quote(this.#key(k))}:${v}`).join(",")}}`;
  }

  #level(rec: LogRecord, name: string, numeric = false): string {
    const style = this.options.levels ?? (numeric ? "number" : "name");
    return style === "number" ? String(rec.level) : quote(name);
  }

  #fields(rec: LogRecord, error: string): Pairs {
    const pairs: Pairs = rec.fields.map(([k, v]) => [k === "error" ? error : k, jsonValue(v)] as const);
    return this.options.nest && pairs.length ? [[this.options.nest, this.#object(pairs)]] : pairs;
  }

  /** `rec` as one JSON object. */
  render(rec: LogRecord): string {
    const m = meta();
    const q = quote;
    const ids: Pairs = (
      [
        ["app", m.app],
        ["env", m.env],
        ["instance", m.instance],
      ] as const
    )
      .filter(([, v]) => v)
      .map(([k, v]) => [k, q(v)] as const);
    const fileLine = rec.file && rec.line !== undefined ? `${rec.file}:${rec.line}` : rec.file;
    const at = <T>(cond: unknown, ...pairs: (readonly [string, T])[]): (readonly [string, T])[] => (cond ? pairs : []);
    const lvl = rec.level;
    let pairs: Pairs;
    switch (this.schema) {
      case "serlog":
        pairs = [
          ["time", q(iso(rec.time))],
          ["level", this.#level(rec, levelName(lvl))],
          ["target", q(rec.target)],
          ["msg", q(rec.msg)],
          ...at(rec.file, ["file", q(rec.file)] as const),
          ...at(rec.line !== undefined, ["line", String(rec.line)] as const),
          ...this.#fields(rec, "error"),
          ...ids,
        ];
        break;
      case "pino":
        pairs = [
          ["level", this.#level(rec, levelName(lvl), true)],
          ["time", String(rec.time.getTime())],
          ["pid", String(process.pid)],
          ["hostname", q(hostName())],
          ...this.#fields(rec, "err"),
          ["msg", q(rec.msg)],
        ];
        break;
      case "bunyan":
        pairs = [
          ["name", q(m.app)],
          ["hostname", q(hostName())],
          ["pid", String(process.pid)],
          ["level", this.#level(rec, levelName(lvl), true)],
          ...this.#fields(rec, "err"),
          ["msg", q(rec.msg)],
          ["time", q(iso(rec.time))],
          ["v", "0"],
        ];
        break;
      case "zap":
        pairs = [
          ["level", this.#level(rec, levelName(lvl))],
          ["ts", seconds(rec.time)],
          ["logger", q(rec.target)],
          ...at(fileLine, ["caller", q(fileLine)] as const),
          ["msg", q(rec.msg)],
          ...this.#fields(rec, "error"),
        ];
        break;
      case "slog":
        pairs = [
          ["time", q(strftime(rec.time, "%Y-%m-%dT%H:%M:%S%.9f%:z"))],
          ["level", this.#level(rec, levelLabel(lvl, "slog"))],
          ["msg", q(rec.msg)],
          ...this.#fields(rec, "error"),
        ];
        break;
      case "structlog":
        pairs = [
          ["event", q(rec.msg)],
          ...this.#fields(rec, "error"),
          ["level", this.#level(rec, levelName(lvl))],
          ["logger", q(rec.target)],
          ["timestamp", q(iso(rec.time))],
        ];
        break;
      case "logstash":
        pairs = [
          ["@timestamp", q(strftime(rec.time, "%Y-%m-%dT%H:%M:%S%.3f%:z"))],
          ["@version", q("1")],
          ["message", q(rec.msg)],
          ["logger_name", q(rec.target)],
          ...at(rec.thread, ["thread_name", q(rec.thread)] as const),
          ["level", this.#level(rec, levelLabel(lvl, "logback"))],
          ["level_value", String(logbackValue(lvl))],
          ...this.#fields(rec, "stack_trace"),
        ];
        break;
      case "ecs":
        pairs = [
          ["@timestamp", q(iso(rec.time))],
          ["log.level", this.#level(rec, levelName(lvl))],
          ["message", q(rec.msg)],
          ["ecs.version", q("8.11.0")],
          ["log.logger", q(rec.target)],
          ...at(rec.file, ["log.origin.file.name", q(rec.file)] as const),
          ...at(rec.line !== undefined, ["log.origin.file.line", String(rec.line)] as const),
          ["process.pid", String(process.pid)],
          ["host.hostname", q(hostName())],
          ...at(m.app, ["service.name", q(m.app)] as const),
          ...at(m.env, ["service.environment", q(m.env)] as const),
          ...at(m.instance, ["service.node.name", q(m.instance)] as const),
          ...this.#fields(rec, "error"),
        ];
        break;
      case "gelf":
        pairs = [
          ["version", q("1.1")],
          ["host", q(hostName())],
          ["short_message", q(rec.msg)],
          ["timestamp", seconds(rec.time)],
          ["level", String(syslog(lvl))],
          ["_target", q(rec.target)],
          ...rec.fields.filter(([k]) => k !== "id").map(([k, v]) => [`_${k}`, jsonValue(v)] as const),
        ];
        break;
      case "otel": {
        const attrs: Pairs = [
          ["code.namespace", q(rec.target)],
          ...at(rec.file, ["code.filepath", q(rec.file)] as const),
          ...at(rec.line !== undefined, ["code.lineno", String(rec.line)] as const),
          ...rec.fields.map(([k, v]) => [k === "error" ? "exception" : k, jsonValue(v)] as const),
        ];
        const res: Pairs = [
          ...at(m.app, ["service.name", q(m.app)] as const),
          ...at(m.env, ["deployment.environment", q(m.env)] as const),
          ...at(m.instance, ["service.instance.id", q(m.instance)] as const),
        ];
        pairs = [
          ["timestamp", q(iso(rec.time, 9))],
          ["severity_text", q(levelLabel(lvl, "serlog"))],
          ["severity_number", String(otelNumber(lvl))],
          ["body", q(rec.msg)],
          ["attributes", this.#object(attrs)],
          ["resource", this.#object(res)],
        ];
        break;
      }
    }
    return this.#object(pairs);
  }
}
