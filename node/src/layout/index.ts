/** How a record becomes text: a pattern, logfmt, a JSON schema, the pretty layout, or a function. */

import type { Theme } from "../colour.ts";
import { type Compat, compat } from "../compat.ts";
import { ConfigError } from "../errors.ts";
import type { LogRecord } from "../record.ts";
import { Json, type Levels, type Schema } from "./json.ts";
import { Pattern } from "./pattern.ts";
import { renderLogfmt, renderPretty } from "./text.ts";

/** How a record becomes one line. */
export interface Layout {
  render(rec: LogRecord, theme?: Theme): string;
}

const profiles = new Map<Compat, Pattern>();

/** The pattern of the compat profile in force. */
export const INHERIT: Layout = {
  render: (rec, theme) => profiles.getOrInsertComputed(compat(), (c) => new Pattern(c)).render(rec, theme),
};

/** `time=… level=… target=… msg=… key=value…` */
export const LOGFMT: Layout = { render: (rec) => renderLogfmt(rec) };

/** A first line plus one line per field, for reading in a terminal. */
export const PRETTY: Layout = { render: (rec, theme) => renderPretty(rec, theme) };

/** A layout from a pattern or preset name, a function, or a layout. */
export function asLayout(x: Layout | string | ((rec: LogRecord) => string)): Layout {
  if (typeof x === "string") return new Pattern(x);
  if (typeof x === "function") return { render: (rec) => x(rec) };
  return x;
}

/** The layout keys of a slot. */
export interface LayoutKeys {
  layout?: Layout | string | ((rec: LogRecord) => string);
  format?: "text" | "logfmt" | "json" | "pretty";
  pattern?: string;
  schema?: Schema;
  keys?: Readonly<Record<string, string>>;
  levels?: Levels;
  nest?: string;
}

let named: (name: string) => Layout = (name) => {
  throw new ConfigError(`unknown layout kind \`${name}\``);
};

/** Lets the config module supply named layouts without an import cycle. */
export function layoutsFrom(find: (name: string) => Layout): void {
  named = find;
}

/** The layout a slot's keys ask for, or `undefined` for the sink's default. */
export function build(k: LayoutKeys): Layout | undefined {
  if (k.layout !== undefined)
    return typeof k.layout === "string" && !k.layout.includes("{") ? named(k.layout) : asLayout(k.layout);
  const jsonish = k.schema !== undefined || k.keys !== undefined || k.levels !== undefined || k.nest !== undefined;
  const opts = {
    ...(k.keys && { keys: k.keys }),
    ...(k.levels && { levels: k.levels }),
    ...(k.nest && { nest: k.nest }),
  };
  if (k.pattern !== undefined && k.format !== undefined && k.format !== "text") {
    throw new ConfigError(`\`pattern\` goes with \`format: text\`, not \`${k.format}\``);
  }
  switch (k.format) {
    case "json":
      return new Json(k.schema ?? "serlog", opts);
    case "logfmt":
      return LOGFMT;
    case "pretty":
      return PRETTY;
    case undefined:
    case "text":
      if (k.pattern !== undefined) return new Pattern(k.pattern);
      return jsonish && k.format === undefined ? new Json(k.schema ?? "serlog", opts) : undefined;
    default:
      throw new ConfigError(`\`format\` takes text, logfmt, json or pretty, not \`${String(k.format)}\``);
  }
}

export { Json, type JsonOptions, type Levels, SCHEMAS, type Schema } from "./json.ts";
export { Pattern, shorten } from "./pattern.ts";
export { jsonValue, logfmt, quote, show } from "./text.ts";
