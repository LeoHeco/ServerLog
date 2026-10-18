/**
 * The hub: global filter, context, global processors, the slot table and dispatch. Like most loggers it starts at
 * `info` and prints to standard output until it is configured.
 */

import { type Alarm, type Desk, settle } from "./alarm/index.ts";
import { wire } from "./alarm/task.ts";
import type { Theme } from "./colour.ts";
import { Claimed, NameTaken, NotReady } from "./errors.ts";
import { asFilter, Filter } from "./filter.ts";
import { inside, suppress } from "./guard.ts";
import type { Layout } from "./layout/index.ts";
import { type Levelish, parseLevel } from "./level.ts";
import { Redact, run, type Step } from "./process/index.ts";
import { changed, type LogRecord } from "./record.ts";
import { bound } from "./scope.ts";
import type { Entry, Sink, SlotOptions } from "./sink.ts";
import { Slot, type Status } from "./slot/index.ts";
import { Live } from "./slot/live.ts";

let filter = new Filter("info");
let floor: number | undefined = 30;
let steps: readonly Step[] = [];
let table: readonly Live[] = [];
let configured = false;
let fallback: Live | undefined;
let reserved: Live | undefined;
let makeConsole: ((stream?: "stderr") => Sink) | undefined;
let hooked = false;

function configure(): void {
  configured = true;
  if (hooked) return;
  hooked = true;
  globalThis.process.once("beforeExit", () => void flush());
}

/** Lets the console module supply the default sinks without an import cycle. */
export function consoleFrom(make: (stream?: "stderr") => Sink): void {
  makeConsole = make;
}

/** Whether a record at `level` could pass the filter for some target; the calls check this first. */
export const enabled = (level: number): boolean => floor !== undefined && level >= floor;

/** The global filter in force. */
export const getFilter = (): Filter => filter;

/** Replaces the global filter; takes effect at once. */
export function setFilter(f: Filter | string | number): void {
  filter = asFilter(f);
  floor = filter.floor;
}

/** Lets through `level` and above for every target. */
export function setLevel(level: Levelish): void {
  setFilter(new Filter(parseLevel(level)));
}

/** Replaces the global processor chain. */
export function setProcess(chain: readonly Step[]): void {
  steps = [...chain];
}

/** Adds a step to the end of the global processor chain. */
export function process(step: Step): void {
  steps = [...steps, step];
}

/** Masks fields with these names, wherever they appear. */
export function redact(...keys: string[]): void {
  process(new Redact({ keys }));
}

/** The global processor chain. */
export const chain = (): readonly Step[] => steps;

/** The registered slots. */
export const slots = (): readonly Live[] => table;

/** Registers a sink with its slot settings at run time; returns the name it got. */
export async function add(sink: Sink | Slot | ((e: Entry) => unknown), options: SlotOptions = {}): Promise<string> {
  const slot = sink instanceof Slot ? sink : new Slot(sink, options);
  const [live] = await prepare([slot], table, new Map());
  if (!live) throw new NameTaken("nothing to add");
  table = [...table, live];
  configure();
  return live.name;
}

/** Takes a slot out, writes what its queue holds and closes its sink. `false` when no slot has that name. */
export async function remove(name: string): Promise<boolean> {
  const gone = table.filter((l) => l.name === name);
  table = table.filter((l) => l.name !== name);
  await Promise.all(gone.map((l) => l.retire()));
  return gone.length > 0;
}

/** Every slot's state. */
export const status = (): Status[] => table.map((l) => l.core.status());

/** Waits up to five seconds for every queue and the alarm route to drain. */
export async function flush(): Promise<void> {
  const until = performance.now() + 5000;
  await flushSlots(5);
  await settle(Math.max(until - performance.now(), 0) / 1000);
}

/** Waits up to `within` seconds for every slot. */
export async function flushSlots(within: number): Promise<void> {
  await Promise.all(table.map((l) => l.flush(within)));
}

/** Sends `rec` through the filter, the context and the global processors, then to every slot on the log route. */
export function emit(rec: LogRecord): void {
  if (suppress()) return;
  if (!filter.allows(rec.level, rec.target) || (filter.hasPattern && !filter.admits(rec.msg))) return;
  const extra = bound();
  let r: LogRecord | null = extra.length ? changed(rec, { fields: [...extra, ...rec.fields] }) : rec;
  if (steps.length) {
    using _ = inside();
    r = run(steps, r);
  }
  if (r) dispatch(r);
}

/** Hands `rec` to every slot on the log route. */
export function dispatch(rec: LogRecord): void {
  const cache = new Map<Layout, Map<Theme | undefined, string>>();
  if (!configured) {
    defaultConsole().emit(rec, cache);
    return;
  }
  for (const l of table) if (!l.alarmOnly) l.emit(rec, cache);
}

function defaultConsole(): Live {
  if (!makeConsole) throw new Error("the console sink is missing");
  fallback ??= new Live(new Slot(makeConsole()), "console", new Map());
  return fallback;
}

/** The reserved console on standard error, where the default alarm goes. */
export function stderrSlot(): Live {
  if (!makeConsole) throw new Error("the console sink is missing");
  reserved ??= new Live(new Slot(makeConsole("stderr"), { onFail: "ignore" }), "stderr", new Map());
  return reserved;
}

/** Names, checks and wraps new slots against those already registered; nothing is kept on error. */
export async function prepare(
  list: readonly Slot[],
  existing: readonly Live[],
  named: ReadonlyMap<string, Alarm>,
): Promise<Live[]> {
  const names = new Set([...existing.map((l) => l.name), "*"]);
  const claims = new Set(existing.flatMap((l) => (l.claim ? [l.claim] : [])));
  const counts = new Map<string, number>();
  for (const l of existing) counts.set(l.core.kind, (counts.get(l.core.kind) ?? 0) + 1);
  const out: Live[] = [];
  for (const slot of list) {
    const kind = slot.sink.kind ?? "custom";
    let name = slot.name;
    while (name === undefined) {
      const n = (counts.get(kind) ?? 0) + 1;
      counts.set(kind, n);
      if (!names.has(`${kind}-${n}`)) name = `${kind}-${n}`;
    }
    if (names.has(name)) throw new NameTaken(`sink name \`${name}\` is taken or reserved`);
    names.add(name);
    const c = slot.sink.claim?.();
    if (c !== undefined) {
      if (claims.has(c)) throw new Claimed(`${c} is already used by another sink`);
      claims.add(c);
    }
    try {
      await slot.sink.ready?.();
    } catch (e) {
      throw new NotReady(name, e);
    }
    out.push(new Live(slot, name, named));
  }
  return out;
}

/** Every name an alarm may route to: registered slots, `stderr` and `*`. */
export const known = (extra: readonly Live[]): Set<string> =>
  new Set([...table, ...extra].map((l) => l.name).concat("stderr", "*"));

/** Puts prepared slots in the table. */
export function install(live: readonly Live[]): void {
  table = [...table, ...live];
  configure();
}

/** Swaps the whole table, for reloading a config. */
export async function replace(live: readonly Live[]): Promise<void> {
  const old = table;
  table = [...live];
  configure();
  await Promise.all(old.map((l) => l.retire()));
}

/** The slot called `name`, the reserved `stderr` included. */
export const find = (name: string): Live | undefined =>
  table.find((l) => l.name === name) ?? (name === "stderr" ? stderrSlot() : undefined);

/** The registered slots, as the alarm walk sees them. */
export const BOARD: Desk = {
  logs: () => table.filter((l) => !l.alarmOnly).map((l) => l.name),
  resting: (name) => find(name)?.core.resting() ?? false,
  deliver: async (name, rec) => {
    const l = find(name);
    if (!l) throw new Error(`no sink called \`${name}\``);
    await l.core.deliver(rec);
  },
};

wire(() => BOARD, flushSlots);
