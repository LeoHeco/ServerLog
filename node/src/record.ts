/** One log event as every sink sees it: frozen, so a processor that changes it makes a copy. */

import { threadId } from "node:worker_threads";

/** Named values attached to a record, in order. */
export type Fields = readonly (readonly [string, unknown])[];

/** A log event. */
export interface LogRecord {
  readonly level: number;
  readonly target: string;
  readonly msg: string;
  readonly fields: Fields;
  readonly tags: readonly string[];
  readonly module?: string;
  readonly func?: string;
  readonly file?: string;
  readonly line?: number;
  readonly thread?: string;
  readonly spans?: string;
  readonly time: Date;
  readonly seq: number;
}

let seq = 0;

/** Everything a record holds beyond its level, target and message. */
export type Extras = Partial<Omit<LogRecord, "level" | "target" | "msg" | "seq">>;

/** A record made now. */
export function record(level: number, target: string, msg: string, extras: Extras = {}): LogRecord {
  return Object.freeze({
    fields: [],
    tags: [],
    thread: threadId === 0 ? "main" : `worker-${threadId}`,
    time: new Date(),
    ...extras,
    level,
    target,
    msg,
    seq: seq++,
  });
}

/** The last field called `key`. */
export const field = (rec: LogRecord, key: string): unknown => rec.fields.findLast(([k]) => k === key)?.[1];

/** A copy with some parts changed. */
export const changed = (rec: LogRecord, parts: Extras & { msg?: string; level?: number }): LogRecord =>
  Object.freeze({ ...rec, ...parts });

/** A copy with `pairs` set, replacing fields of the same name. */
export function withFields(rec: LogRecord, pairs: Iterable<readonly [string, unknown]>): LogRecord {
  const add = new Map(pairs);
  return changed(rec, { fields: [...rec.fields.filter(([k]) => !add.has(k)), ...add] });
}

/** An object's own entries as fields. */
export const fieldsOf = (o: object): Fields => Object.entries(o);
