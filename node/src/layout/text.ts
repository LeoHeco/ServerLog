/** Text pieces shared by the layouts: quoting, values, field lists, logfmt and the pretty layout. */

import { type Colour, paint, type Theme } from "../colour.ts";
import { compat } from "../compat.ts";
import { levelColour, levelLabel, levelName } from "../level.ts";
import type { Fields, LogRecord } from "../record.ts";
import { iso, writeTime } from "./clock.ts";

const ESCAPES: Readonly<Record<string, string>> = { '"': '\\"', "\\": "\\\\", "\n": "\\n", "\r": "\\r", "\t": "\\t" };

/** `v` as a JSON string, escaped as every serlog package escapes it. */
export function quote(v: unknown): string {
  let out = '"';
  for (const c of String(v)) {
    out += ESCAPES[c] ?? (c < " " ? `\\u${c.charCodeAt(0).toString(16).padStart(4, "0")}` : c);
  }
  return `${out}"`;
}

/** `v` bare, or quoted when it is empty or holds whitespace, quotes or `=`. */
export function logfmt(v: unknown): string {
  const s = show(v);
  return s && !/[\s"=]/.test(s) ? s : quote(s);
}

const isMap = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" &&
  v !== null &&
  !Array.isArray(v) &&
  !Error.isError(v) &&
  !(v instanceof Date) &&
  !(v instanceof Map);

/** What `e` was caused by, then what that was caused by, and so on. */
export function* causes(e: Error): Generator<unknown> {
  const seen = new Set<unknown>([e]);
  for (let c = e.cause; c !== undefined && !seen.has(c); c = Error.isError(c) ? c.cause : undefined) {
    seen.add(c);
    yield c;
  }
}

const text = (v: unknown): string => (Error.isError(v) ? v.message : String(v));

/** A value as text: `null`, `true`, `[a, b]`, `{k=v}`, errors with their causes. */
export function show(v: unknown): string {
  if (v === null || v === undefined) return "null";
  if (typeof v === "string") return v;
  if (Error.isError(v)) return [v, ...causes(v)].map(text).join(": ");
  if (Array.isArray(v) || v instanceof Set) return `[${[...v].map(show).join(", ")}]`;
  if (v instanceof Map) return `{${[...v].map(([k, x]) => `${k}=${show(x)}`).join(", ")}}`;
  if (v instanceof Date) return v.toISOString();
  if (isMap(v))
    return `{${Object.entries(v)
      .map(([k, x]) => `${k}=${show(x)}`)
      .join(", ")}}`;
  return String(v);
}

/** A value as JSON. */
export function jsonValue(v: unknown): string {
  if (v === null || v === undefined) return "null";
  switch (typeof v) {
    case "boolean":
      return String(v);
    case "number":
      return Number.isFinite(v) ? String(v) : quote(v);
    case "bigint":
      return String(v);
    case "string":
      return quote(v);
  }
  if (Error.isError(v)) {
    const parts = [`"message":${quote(v.message)}`];
    const c = [...causes(v)];
    if (c.length) parts.push(`"causes":[${c.map((x) => quote(text(x))).join(",")}]`);
    if (v.stack) parts.push(`"stack":${quote(v.stack)}`);
    return `{${parts.join(",")}}`;
  }
  if (Array.isArray(v) || v instanceof Set) return `[${[...v].map(jsonValue).join(",")}]`;
  if (v instanceof Map) return `{${[...v].map(([k, x]) => `${quote(k)}:${jsonValue(x)}`).join(",")}}`;
  if (v instanceof Date) return quote(v.toISOString());
  if (isMap(v))
    return `{${Object.entries(v)
      .map(([k, x]) => `${quote(k)}:${jsonValue(x)}`)
      .join(",")}}`;
  return quote(v);
}

/** Each field, plain objects spread into `key.sub`. */
export function* flat(fields: Fields, skip: readonly string[] = []): Generator<readonly [string, unknown]> {
  function* walk(key: string, v: unknown): Generator<readonly [string, unknown]> {
    if (isMap(v)) for (const [k, x] of Object.entries(v)) yield* walk(`${key}.${k}`, x);
    else yield [key, v];
  }
  for (const [k, v] of fields) if (!skip.includes(k)) yield* walk(k, v);
}

/** ` key=value` per field. */
export const fieldText = (rec: LogRecord, quoted: boolean, skip: readonly string[] = []): string =>
  flat(rec.fields, skip)
    .map(([k, v]) => ` ${k}=${quoted ? logfmt(v) : show(v)}`)
    .toArray()
    .join("");

/** `time=… level=… target=… msg=… key=value…` */
export const renderLogfmt = (rec: LogRecord): string =>
  `time=${iso(rec.time, 0)} level=${levelName(rec.level)} target=${logfmt(rec.target)} msg=${logfmt(rec.msg)}${fieldText(rec, true)}`;

/** A first line with time, level, target and message, then one aligned line per field. */
export function renderPretty(rec: LogRecord, theme: Theme | undefined): string {
  const on = theme !== undefined;
  const colour: Colour = theme ? theme.of(rec.level, levelColour(rec.level)) : "plain";
  const head = `${paint(on, "grey", writeTime("", rec.time))} ${paint(on, colour, levelLabel(rec.level, compat()).padEnd(5))} ${paint(on, "white", rec.target)} ${rec.msg}`;
  const rows = flat(rec.fields)
    .map(([k, v]) => [k, show(v)] as const)
    .toArray();
  const width = Math.max(0, ...rows.map(([k]) => k.length));
  return head + rows.map(([k, v]) => `\n    ${paint(on, "cyan", k.padEnd(width))} = ${v}`).join("");
}
