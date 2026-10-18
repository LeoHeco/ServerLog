/** What happens when a sink fails: panic, ignore, or raise an alarm that walks tiers of other sinks. */

import { iso } from "../layout/clock.ts";
import { Level } from "../level.ts";
import { type LogRecord, record } from "../record.ts";
import type { Phase } from "../slot/breaker.ts";
import { seconds, showTime } from "../units.ts";

/** What an alarm does when no tier took the incident. */
export type Fallback = "panic" | "ignore";

/** One failed write. */
export interface Failure {
  readonly sink: string;
  readonly error: string;
  readonly time: Date;
}

/** A sink's failure, from the first error to recovery. */
export interface Incident {
  readonly sink: string;
  readonly kind: string;
  readonly phase: Phase;
  readonly error: string;
  readonly failures: Failure[];
  readonly skipped: number;
  readonly dropped: number;
  readonly since: Date;
}

/** A fresh incident; its failures start with the sink's own unless it is resolved. */
export function incident(
  sink: string,
  kind: string,
  phase: Phase,
  error: string,
  more: Partial<Incident> = {},
): Incident {
  const failures = more.failures ?? (phase === "resolved" ? [] : [{ sink, error, time: new Date() }]);
  return { skipped: 0, dropped: 0, since: new Date(), ...more, sink, kind, phase, error, failures };
}

/** The default record for an incident. */
export function describe(i: Incident): LogRecord {
  const gone = Math.floor((Date.now() - i.since.getTime()) / 1000);
  const [level, msg] =
    i.phase === "opened"
      ? [Level.ERROR, `sink \`${i.sink}\` failed: ${i.error}`]
      : i.phase === "reminder"
        ? [Level.ERROR, `sink \`${i.sink}\` is still failing: ${i.error}`]
        : [Level.INFO, `sink \`${i.sink}\` works again after ${gone}s`];
  return record(level, "serlog", msg, {
    fields: [
      ["sink", i.sink],
      ["kind", i.kind],
      ["phase", i.phase],
      ["failures", i.failures.map((f) => `${f.sink}: ${f.error}`).join("; ")],
      ["skipped", i.skipped],
      ["dropped", i.dropped],
      ["since", iso(i.since, 9)],
    ],
  });
}

/**
 * Routes incidents to sinks. Extend it and override `route` to choose the tiers; the alarm task walks them, leaves
 * out the failing sink and any that are resting, delivers within a tier at once and stops at the first tier where
 * one sink took it. Subclasses cannot cause loops or storms.
 */
export abstract class Alarm {
  /** What to do when no tier took it. */
  fallback: Fallback = "ignore";
  /** Seconds a sink may stay down before it is reported again. */
  remind = 3600;

  /** The tiers for this incident, tried in order; `*` stands for every sink on the log route. */
  abstract route(incident: Incident): string[][];

  /** The record sent; asked again before each tier, so later tiers see earlier failures. */
  describe(incident: Incident): LogRecord {
    return describe(incident);
  }

  /** Every sink name this alarm may route to, checked at start-up. */
  names(): string[] {
    return [];
  }

  /** The config entry that rebuilds this alarm, or `undefined` when it cannot be exported. */
  options(): Record<string, unknown> | undefined {
    return undefined;
  }
}

/** Options of [`Tiers`]. */
export interface TiersOptions {
  tiers?: readonly (readonly string[] | string)[];
  onFail?: Fallback;
  remind?: number | string;
}

/** The built-in alarm: fixed tiers of sink names. */
export class Tiers extends Alarm {
  readonly tiers: readonly (readonly string[])[];

  constructor({ tiers = [["stderr"]], onFail = "ignore", remind = 3600 }: TiersOptions = {}) {
    super();
    this.tiers = tiers.map((t) => (typeof t === "string" ? [t] : [...t]));
    if (!this.tiers.length || this.tiers.some((t) => !t.length))
      throw new TypeError("`tiers` needs at least one non-empty tier");
    if (onFail !== "panic" && onFail !== "ignore")
      throw new TypeError(`an alarm's \`on_fail\` is panic or ignore, not \`${String(onFail)}\``);
    this.fallback = onFail;
    this.remind = seconds(remind);
  }

  route(): string[][] {
    return this.tiers.map((t) => [...t]);
  }

  override names(): string[] {
    return this.tiers.flat();
  }

  override options(): Record<string, unknown> {
    return {
      tiers: this.route(),
      ...(this.fallback === "panic" && { on_fail: "panic" }),
      ...(this.remind !== 3600 && { remind: showTime(this.remind) }),
    };
  }
}

let fallbackAlarm: Alarm | undefined;

/** Replaces the process-wide default alarm. */
export function setAlarm(alarm: Alarm): void {
  fallbackAlarm = alarm;
}

/** The default alarm in force, made on first use. */
export const current = (): Alarm => (fallbackAlarm ??= new Tiers());

class Current extends Alarm {
  route(i: Incident): string[][] {
    return current().route(i);
  }

  override describe(i: Incident): LogRecord {
    return current().describe(i);
  }

  override names(): string[] {
    return current().names();
  }

  override options(): Record<string, unknown> | undefined {
    return current().options();
  }
}

/** What every slot without an `onFail` reports to. */
export const DEFAULT: Alarm = new Current();

/** The alarm in force behind `alarm`, which differs only for [`DEFAULT`]. */
export const resolve = (alarm: Alarm): Alarm => (alarm === DEFAULT ? current() : alarm);

export { crash, dropped, post, settle } from "./task.ts";
export { type Desk, type Outcome, sound } from "./walk.ts";
