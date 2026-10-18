/**
 * Steps a record goes through before it is written: return it, a changed copy, or `null` to drop it. Records are
 * frozen, so a step that changes one returns a copy and every other slot still sees the original.
 */

import type { LogRecord } from "../record.ts";

/** A built-in or custom step that can also describe itself for export. */
export interface Processor {
  readonly kind: string;
  process(rec: LogRecord): LogRecord | null;
  options?(): Record<string, unknown>;
}

/** A processor, or any function taking a record. */
export type Step = Processor | ((rec: LogRecord) => LogRecord | null | undefined);

/** Whether `x` is a step rather than a processor's name or config entry. */
export const isStep = (x: unknown): x is Step =>
  typeof x === "function" || (typeof x === "object" && x !== null && "process" in x);

/** `rec` after every step, or `null` once one dropped it. */
export function run(steps: readonly Step[], rec: LogRecord): LogRecord | null {
  let cur: LogRecord | null = rec;
  for (const s of steps) {
    cur = (typeof s === "function" ? s(cur) : s.process(cur)) ?? null;
    if (!cur) return null;
  }
  return cur;
}

export { Dedupe, Exclude, Fields, Limit, Match, Redact, Sample, Sanitise } from "./builtin.ts";
