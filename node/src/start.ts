/** Start-up: one call, the same keys as a config file, all checked before anything changes. */

import { existsSync } from "node:fs";
import { Alarm, DEFAULT, setAlarm } from "./alarm/index.ts";
import { debugDirectives } from "./bridge.ts";
import { type Colour, parseColour } from "./colour.ts";
import { type Compat, envOf, levelOf, parseCompat, setCompat } from "./compat.ts";
import { nameAlarms } from "./config/export.ts";
import { camel, load, makeAlarm, makeSink, makeStep, snake } from "./config/index.ts";
import { ConfigError, Twice } from "./errors.ts";
import { asFilter, Filter } from "./filter.ts";
import * as hub from "./hub.ts";
import type { LayoutKeys } from "./layout/index.ts";
import { defineLevel, type Levelish, parseLevel } from "./level.ts";
import { detect, type Meta, setMeta } from "./meta.ts";
import { isStep, Redact, type Step } from "./process/index.ts";
import type { Sink } from "./sink.ts";
import { Console } from "./sinks/console.ts";
import { Slot } from "./slot/index.ts";
import type { Live } from "./slot/live.ts";

/** What `init` takes: the keys of a config file, in camel case. */
export interface Settings {
  app?: string;
  env?: string;
  instance?: string;
  compat?: Compat | string;
  level?: Levelish;
  filter?: string | Filter;
  filterEnv?: string;
  levels?: Readonly<Record<string, number | { value: number; colour?: Colour }>>;
  process?: readonly (Step | string | Readonly<Record<string, unknown>>)[];
  redact?: readonly string[];
  sinks?: readonly (Sink | Slot | Readonly<Record<string, unknown>>)[];
  alarms?: Readonly<Record<string, Alarm | Readonly<Record<string, unknown>>>>;
  watch?: number | string;
}

const KEYS = new Set([
  "app",
  "env",
  "instance",
  "compat",
  "level",
  "filter",
  "filterEnv",
  "levels",
  "process",
  "redact",
  "sinks",
  "alarms",
  "watch",
]);
const FORMATS = new Set(["text", "json", "logfmt", "pretty"]);

/** Everything checked and built, ready to swap in. */
export interface Plan {
  readonly meta: Meta;
  readonly compat: Compat;
  readonly filter: Filter;
  readonly process: readonly Step[];
  readonly live: readonly Live[];
  readonly alarm: Alarm | undefined;
  readonly named: ReadonlyMap<string, Alarm>;
}

function asSlot(s: Sink | Slot | Readonly<Record<string, unknown>>): Slot {
  if (s instanceof Slot) return s;
  return new Slot(typeof s.emit === "function" ? (s as Sink) : makeSink(s as Readonly<Record<string, unknown>>));
}

function console(): Slot {
  const format = process.env.SERLOG_FORMAT || undefined;
  if (format !== undefined && !FORMATS.has(format))
    throw new ConfigError(`SERLOG_FORMAT \`${format}\` is not text, json, logfmt or pretty`);
  return new Slot(new Console(), format ? { format: format as NonNullable<LayoutKeys["format"]> } : {});
}

/** `filter` with the `DEBUG` namespaces added, ahead of any `/regex` part. */
function withDebug(filter: Filter): Filter {
  const extra = debugDirectives(process.env.DEBUG ?? "");
  if (!extra) return filter;
  const text = String(filter);
  const cut = text.indexOf("/");
  const [head, tail] = cut < 0 ? [text, ""] : [text.slice(0, cut), text.slice(cut)];
  return new Filter(`${[head, extra].filter(Boolean).join(",")}${tail}`);
}

/** Checks `cfg` and builds its slots, keeping nothing on error. */
export async function plan(
  given: Settings | Readonly<Record<string, unknown>>,
  alongside: readonly Live[],
): Promise<Plan> {
  const cfg: Settings = Object.fromEntries(Object.entries(given).map(([k, v]) => [camel(k), v]));
  const bad = Object.keys(cfg).find((k) => !KEYS.has(k));
  if (bad !== undefined) throw new ConfigError(`unknown key \`${snake(bad)}\``);
  for (const [name, spec] of Object.entries(cfg.levels ?? {})) {
    const [value, colour] = typeof spec === "number" ? [spec, "plain"] : [spec.value, spec.colour ?? "plain"];
    defineLevel(name, value, parseColour(colour));
  }
  const profile = parseCompat(process.env.SERLOG_COMPAT?.trim() || cfg.compat || "serlog");
  const found = (cfg.filterEnv ? [cfg.filterEnv] : envOf(profile)).map((n) => process.env[n]?.trim()).find(Boolean);
  const spec = found || cfg.filter;
  const filter = withDebug(spec !== undefined ? asFilter(spec) : new Filter(parseLevel(cfg.level ?? levelOf(profile))));
  const steps = (cfg.process ?? []).map((s) => (isStep(s) ? s : makeStep(s)));
  if (cfg.redact?.length) steps.push(new Redact({ keys: [...cfg.redact] }));
  const named = new Map(
    Object.entries(cfg.alarms ?? {}).map(([n, a]) => [n, a instanceof Alarm ? a : makeAlarm(n, a)] as const),
  );
  const fallback = named.get("default");
  named.delete("default");
  const slots = (cfg.sinks ?? []).map(asSlot);
  const live = await hub.prepare(slots.length ? slots : [console()], alongside, named);
  const known = hub.known(live);
  const routes: (readonly [string, unknown])[] = [
    ["the default alarm", fallback],
    ...live.map((l) => [`the alarm of \`${l.name}\``, l.core.onFail] as const),
  ];
  for (const [whose, a] of routes) {
    const miss = a instanceof Alarm && a !== DEFAULT ? a.names().find((n) => !known.has(n)) : undefined;
    if (miss !== undefined) throw new ConfigError(`${whose} routes to \`${miss}\`, which is not a sink`);
  }
  const m = detect();
  return {
    meta: { app: cfg.app ?? m.app, env: cfg.env ?? m.env, instance: cfg.instance ?? m.instance },
    compat: profile,
    filter,
    process: steps,
    live,
    alarm: fallback,
    named,
  };
}

/** Puts a plan in force, adding to the slots or replacing them. */
export function apply(p: Plan, replace = false): void {
  setMeta(p.meta);
  setCompat(p.compat);
  hub.setFilter(p.filter);
  hub.setProcess(p.process);
  if (p.alarm) setAlarm(p.alarm);
  nameAlarms(p.named);
  if (replace) void hub.replace(p.live);
  else hub.install(p.live);
}

let started = false;

/** Whether `init` ran. */
export const isStarted = (): boolean => started;

/** Starts from settings that are already in hand, as `load` does. */
export async function start(cfg: Settings | Readonly<Record<string, unknown>>): Promise<void> {
  if (started) throw new Twice();
  started = true;
  try {
    apply(await plan(cfg, hub.slots()));
  } catch (e) {
    started = false;
    throw e;
  }
}

/**
 * Starts serlog once, from settings that are also the keys of a config file. With none it reads `SERLOG_CONFIG`,
 * or the first of `serlog.yaml`, `serlog.yml`, `serlog.json` and `serlog.toml` in the working directory; failing
 * that it starts a console sink.
 */
export async function init(cfg: Settings = {}): Promise<void> {
  if (started) throw new Twice();
  const path = Object.keys(cfg).length
    ? undefined
    : process.env.SERLOG_CONFIG ||
      ["serlog.yaml", "serlog.yml", "serlog.json", "serlog.toml"].find((n) => existsSync(n));
  await (path ? load(path) : start(cfg));
}
