/** The settings in force as a config tree. */

import { type Alarm, current, DEFAULT, Tiers } from "../alarm/index.ts";
import { compat } from "../compat.ts";
import { ConfigError } from "../errors.ts";
import { chain, getFilter, slots } from "../hub.ts";
import { Json, LOGFMT, Pattern, PRETTY } from "../layout/index.ts";
import { definedLevels, levelName } from "../level.ts";
import { meta } from "../meta.ts";
import type { Step } from "../process/index.ts";
import type { Live } from "../slot/live.ts";
import { showTime } from "../units.ts";

let named = new Map<string, Alarm>();

/** Remembers the names alarms were given in config. */
export function nameAlarms(m: ReadonlyMap<string, Alarm>): void {
  named = new Map(m);
}

function steps(list: readonly Step[], whose: string): unknown[] {
  return list.map((s) => {
    const o = typeof s === "function" ? undefined : s.options?.();
    if (!o) throw new ConfigError(`a processor in ${whose} cannot be exported`);
    return o;
  });
}

function layoutKeys(l: Live): Record<string, unknown> {
  const lay = l.slot.layout;
  if (!lay) return {};
  if (lay instanceof Pattern) return { pattern: lay.source };
  if (lay === LOGFMT) return { format: "logfmt" };
  if (lay === PRETTY) return { format: "pretty" };
  if (lay instanceof Json) {
    const o = lay.options;
    return {
      format: "json",
      ...(lay.schema !== "serlog" && { schema: lay.schema }),
      ...(o.keys && { keys: o.keys }),
      ...(o.levels && { levels: o.levels }),
      ...(o.nest && { nest: o.nest }),
    };
  }
  throw new ConfigError(`the layout of sink \`${l.name}\` cannot be exported`);
}

function onFail(p: unknown, extra: Map<string, Alarm>): string | undefined {
  if (p === "panic" || p === "ignore") return p;
  if (p === DEFAULT) return undefined;
  for (const [n, a] of [...named, ...extra]) if (a === p) return n;
  const n = `alarm-${extra.size + 1}`;
  extra.set(n, p as Alarm);
  return n;
}

function sink(l: Live, extra: Map<string, Alarm>): Record<string, unknown> {
  const c = l.core;
  const own = c.sink.options?.();
  if (!own) throw new ConfigError(`sink \`${l.name}\` cannot be exported`);
  const s = l.slot;
  const theme = [...s.theme.overrides];
  const policy = onFail(c.onFail, extra);
  const q = l.queue;
  return {
    kind: c.kind,
    name: l.name,
    ...own,
    ...(s.filter && { filter: String(s.filter) }),
    ...(s.process.length && { process: steps(s.process, `sink \`${l.name}\``) }),
    ...layoutKeys(l),
    ...(s.colour !== "auto" && { colour: s.colour }),
    ...(theme.length && { colours: Object.fromEntries(theme.map(([lv, col]) => [levelName(lv), col])) }),
    ...(s.alarmOnly && { alarm_only: true }),
    ...(policy && { on_fail: policy }),
    ...(s.mode !== "sink" &&
      (q
        ? { queue: q.size, batch: q.batch, wait: showTime(q.wait), ...(q.full === "block" && { full: "block" }) }
        : { queue: 0 })),
    ...(s.retries !== undefined && { retries: s.retries }),
    ...(s.probe !== 5 && { probe: showTime(s.probe) }),
  };
}

/** The settings in force. */
export function tree(): Record<string, unknown> {
  const m = meta();
  const levels = [...definedLevels()].map(([v, [n, c]]) => [n, c === "plain" ? v : { value: v, colour: c }] as const);
  const extra = new Map<string, Alarm>();
  const sinks = slots().map((l) => sink(l, extra));
  const alarms: Record<string, unknown> = {};
  const def = current().options();
  if (!def) throw new ConfigError("the default alarm cannot be exported");
  if (JSON.stringify(def) !== JSON.stringify(new Tiers().options())) alarms.default = def;
  for (const [n, a] of [...named, ...extra]) {
    const o = a.options();
    if (!o) throw new ConfigError(`alarm \`${n}\` cannot be exported`);
    alarms[n] = o;
  }
  const process = steps(chain(), "the global chain");
  return {
    ...(m.app && { app: m.app }),
    ...(m.env && { env: m.env }),
    ...(m.instance && { instance: m.instance }),
    ...(compat() !== "serlog" && { compat: compat() }),
    ...(levels.length && { levels: Object.fromEntries(levels) }),
    filter: String(getFilter()),
    ...(process.length && { process }),
    sinks,
    ...(Object.keys(alarms).length && { alarms }),
  };
}
