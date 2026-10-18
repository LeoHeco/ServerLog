/** The four kinds of extension a config file can name: sinks, processors, layouts and alarms. */

import { type Alarm, Tiers, type TiersOptions } from "../alarm/index.ts";
import { KindError } from "../errors.ts";
import type { Layout } from "../layout/index.ts";
import { Dedupe, Exclude, Fields, Limit, Match, Redact, Sample, Sanitise, type Step } from "../process/index.ts";
import type { Sink } from "../sink.ts";
import {
  Console,
  File,
  type FileOptions,
  Loki,
  type LokiOptions,
  Mail,
  type MailOptions,
  Memory,
} from "../sinks/index.ts";

/** A factory and, for built-ins, the keys it takes besides the slot's. */
interface Kind<T> {
  readonly make: (options: Record<string, unknown>) => T;
  readonly keys?: ReadonlySet<string>;
}

const keys = (...k: string[]): ReadonlySet<string> => new Set(k);

const sinks = new Map<string, Kind<Sink>>([
  ["console", { make: (o) => new Console(o), keys: keys("stream") }],
  [
    "file",
    {
      make: (o) => new File(o as unknown as FileOptions),
      keys: keys(
        "path",
        "append",
        "perms",
        "mkdir",
        "maxSize",
        "rotate",
        "keep",
        "maxAge",
        "maxTotal",
        "gzip",
        "rollName",
        "reopen",
        "buffer",
        "flushEvery",
      ),
    },
  ],
  ["loki", { make: (o) => new Loki(o as unknown as LokiOptions), keys: keys("url", "labels", "timeout", "connect") }],
  [
    "mail",
    {
      make: (o) => new Mail(o as unknown as MailOptions),
      keys: keys("host", "to", "port", "security", "user", "pass", "password", "from", "sender", "timeout", "connect"),
    },
  ],
  ["memory", { make: (o) => new Memory(o), keys: keys("size") }],
]);

const conditions = keys("tags", "fields", "level");
const steps = new Map<string, Kind<Step>>([
  ["redact", { make: (o) => new Redact(o), keys: keys("keys", "paths", "patterns", "mask") }],
  ["sanitise", { make: () => new Sanitise(), keys: keys() }],
  ["fields", { make: (o) => new Fields(o) }],
  ["match", { make: (o) => new Match(o), keys: conditions }],
  ["exclude", { make: (o) => new Exclude(o), keys: conditions }],
  ["sample", { make: (o) => new Sample(o), keys: keys("first", "every", "per") }],
  ["limit", { make: (o) => new Limit(o as { rate: number }), keys: keys("rate", "burst") }],
  ["dedupe", { make: (o) => new Dedupe(o), keys: keys("within") }],
]);

const layouts = new Map<string, () => Layout>();
const alarms = new Map<string, Kind<Alarm>>([
  ["tiers", { make: (o) => new Tiers(o as TiersOptions), keys: keys("tiers", "onFail", "remind") }],
]);

/** Lets config files name a kind of sink. */
export function kind(name: string, make: (options: Record<string, unknown>) => Sink): void {
  sinks.set(name, { make });
}

/** Lets config files name a processor in `process`. */
export function processor(name: string, make: (options: Record<string, unknown>) => Step): void {
  steps.set(name, { make });
}

/** Lets config files name a layout with `layout: name`. */
export function layout(name: string, make: () => Layout): void {
  layouts.set(name, make);
}

/** Lets config files name a kind of alarm with `kind: name` under `alarms`. */
export function alarm(name: string, make: (options: Record<string, unknown>) => Alarm): void {
  alarms.set(name, { make });
}

const find = <T>(table: ReadonlyMap<string, T>, name: string, what: string): T => {
  const k = table.get(name);
  if (!k) throw new KindError(`unknown ${what} kind \`${name}\``);
  return k;
};

/** The factory for a sink kind. */
export const sinkKind = (name: string): Kind<Sink> => find(sinks, name, "sink");
/** The factory for a processor kind. */
export const stepKind = (name: string): Kind<Step> => find(steps, name, "processor");
/** The factory for a named layout. */
export const layoutKind = (name: string): (() => Layout) => find(layouts, name, "layout");
/** The factory for an alarm kind. */
export const alarmKind = (name: string): Kind<Alarm> => find(alarms, name, "alarm");
