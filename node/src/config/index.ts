/**
 * Start-up from YAML, JSON or TOML, export of the settings in force, and reloading when the file changes. The keys
 * are those `serlog.init` takes, in camel case in code and snake case in files.
 */

import { readFileSync, statSync } from "node:fs";
import { extname } from "node:path";
import type { Alarm } from "../alarm/index.ts";
import { ConfigError } from "../errors.ts";
import { BOARD } from "../hub.ts";
import { layoutsFrom } from "../layout/index.ts";
import { Level } from "../level.ts";
import type { Step } from "../process/index.ts";
import { record } from "../record.ts";
import type { Sink } from "../sink.ts";
import { SLOT_KEYS } from "../sink.ts";
import { stepsFrom } from "../slot/index.ts";
import { apply, plan, start } from "../start.ts";
import { seconds } from "../units.ts";
import { tree as current } from "./export.ts";
import { alarmKind, layoutKind, sinkKind, stepKind } from "./kinds.ts";

/** Config file syntax. */
export type Format = "yaml" | "json" | "toml";

/** The format a file extension names; YAML for anything but `.json` and `.toml`. */
export const formatOf = (path: string): Format =>
  (({ ".json": "json", ".toml": "toml" })[extname(path).toLowerCase()] as Format) ?? "yaml";

/** `alarm_only` as `alarmOnly`. */
export const camel = (k: string): string => k.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());

/** `alarmOnly` as `alarm_only`. */
export const snake = (k: string): string => k.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);

const camelKeys = (o: Readonly<Record<string, unknown>>): Record<string, unknown> =>
  Object.fromEntries(Object.entries(o).map(([k, v]) => [camel(k), v]));

/** Config text as a tree, without expanding variables. */
export async function read(text: string, format: Format = "yaml"): Promise<Record<string, unknown>> {
  let tree: unknown;
  switch (format) {
    case "json":
      tree = JSON.parse(text);
      break;
    case "toml":
      tree = structuredClone((await import("smol-toml")).parse(text));
      break;
    default:
      tree = (await import("yaml")).parse(text);
  }
  if (tree === null || tree === undefined) return {};
  if (typeof tree !== "object" || Array.isArray(tree)) throw new ConfigError("the config must be a map");
  return tree as Record<string, unknown>;
}

/** `${NAME}` in every string replaced by that environment variable; an unset one is an error. */
export function expand<T>(tree: T): T {
  if (typeof tree === "string") {
    return tree.replace(/\$\{([^}]*)\}/g, (_, name: string) => {
      const v = process.env[name];
      if (v === undefined) throw new ConfigError(`environment variable ${name} is not set`);
      return v;
    }) as T;
  }
  if (Array.isArray(tree)) return tree.map(expand) as T;
  if (tree && typeof tree === "object" && !(tree instanceof Date)) {
    return Object.fromEntries(Object.entries(tree).map(([k, v]) => [k, expand(v)])) as T;
  }
  return tree;
}

function check(
  what: string,
  opts: Readonly<Record<string, unknown>>,
  allowed: ReadonlySet<string> | undefined,
  extra: ReadonlySet<string> = new Set(),
): void {
  if (!allowed) return;
  const bad = Object.keys(opts).find((k) => !allowed.has(k) && !extra.has(k));
  if (bad !== undefined) throw new ConfigError(`${what}: unknown key \`${snake(bad)}\``);
}

/** The sink a config entry describes, slot keys included. */
export function makeSink(entry: Readonly<Record<string, unknown>>): Sink {
  const { kind, ...rest } = camelKeys(entry);
  if (typeof kind !== "string") throw new ConfigError("each sink needs a `kind`");
  const k = sinkKind(kind);
  check(`${kind} sink`, rest, k.keys, SLOT_KEYS);
  try {
    return k.make(rest);
  } catch (e) {
    throw e instanceof ConfigError ? e : new ConfigError(`${kind} sink: ${(e as Error).message}`, { cause: e });
  }
}

/** The processor a `process` entry describes: a name, or a map from a name to its options. */
export function makeStep(spec: string | Readonly<Record<string, unknown>>): Step {
  const [name, value] = typeof spec === "string" ? [spec, {}] : (Object.entries(spec)[0] ?? ["", {}]);
  if (typeof spec !== "string" && Object.keys(spec).length !== 1)
    throw new ConfigError(`a processor is a name or a one-key map`);
  const opts = value === true || value === null ? {} : value;
  if (typeof opts !== "object") throw new ConfigError(`processor \`${name}\` takes a map`);
  const k = stepKind(name);
  check(`processor \`${name}\``, opts as Record<string, unknown>, k.keys);
  return k.make(opts as Record<string, unknown>);
}

/** The alarm an `alarms` entry describes. */
export function makeAlarm(name: string, spec: Readonly<Record<string, unknown>>): Alarm {
  if (name === "panic" || name === "ignore") throw new ConfigError(`\`${name}\` cannot name an alarm`);
  const { kind = "tiers", ...rest } = camelKeys(spec);
  const k = alarmKind(String(kind));
  check(`alarm \`${name}\``, rest, k.keys);
  try {
    return k.make(rest);
  } catch (e) {
    throw new ConfigError(`alarm \`${name}\`: ${(e as Error).message}`, { cause: e });
  }
}

stepsFrom(makeStep);
layoutsFrom((name) => layoutKind(name)());

/** Initialises from config text. */
export async function loads(text: string, format: Format = "yaml"): Promise<void> {
  await start(expand(await read(text, format)));
}

/** Initialises from a file, the format from its extension; reloads it if it sets `watch`. */
export async function load(path: string): Promise<void> {
  const tree = expand(await read(readFileSync(path, "utf8"), formatOf(path)));
  await start(tree);
  if (tree.watch !== undefined) watch(path, tree.watch as number | string);
}

/** Reloads `path` whenever its modification time changes, if all of it passes. */
export function watch(path: string, every: number | string): void {
  const stamp = (): number | undefined => {
    try {
      return statSync(path).mtimeMs;
    } catch {
      return undefined;
    }
  };
  let seen = stamp();
  setInterval(async () => {
    const now = stamp();
    if (now === seen) return;
    seen = now;
    try {
      apply(await plan(expand(await read(readFileSync(path, "utf8"), formatOf(path))), []), true);
    } catch (e) {
      const rec = record(Level.ERROR, "serlog", `reloading ${path} failed: ${(e as Error).message}`);
      await Promise.resolve(BOARD.deliver("stderr", rec)).catch(() => process.stderr.write(`${rec.msg}\n`));
    }
  }, seconds(every) * 1000).unref();
}

/** The settings in force as config text that loads back to the same behaviour; passwords come out as `***`. */
export async function dump(format: Format = "yaml"): Promise<string> {
  const t = current();
  switch (format) {
    case "json":
      return `${JSON.stringify(t, null, 2)}\n`;
    case "toml":
      return (await import("smol-toml")).stringify(t);
    default:
      return (await import("yaml")).stringify(t);
  }
}

export { alarm, kind, layout, processor } from "./kinds.ts";
