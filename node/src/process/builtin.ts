/** The built-in steps: redact, sanitise, fields, match, exclude, sample, limit and dedupe. */

import { show } from "../layout/text.ts";
import { type Levelish, levelName, parseLevel } from "../level.ts";
import { changed, field, type LogRecord, withFields } from "../record.ts";
import { seconds, showTime } from "../units.ts";
import type { Processor } from "./index.ts";

const isMap = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v) && !Error.isError(v) && !(v instanceof Date);

/** Options of [`Redact`]. */
export interface RedactOptions {
  keys?: readonly string[];
  paths?: readonly string[];
  patterns?: readonly string[];
  mask?: string;
}

/** Masks fields by name at any depth or by dotted path, and parts of the message by regex. */
export class Redact implements Processor {
  readonly kind = "redact";
  readonly #keys: ReadonlySet<string>;
  readonly #paths: readonly string[];
  readonly #patterns: readonly RegExp[];
  readonly #mask: string;

  constructor({ keys = [], paths = [], patterns = [], mask = "***" }: RedactOptions = {}) {
    this.#keys = new Set(keys);
    this.#paths = paths;
    this.#patterns = patterns.map((p) => new RegExp(p, "g"));
    this.#mask = mask;
  }

  #scrub(path: string, key: string, v: unknown): unknown {
    if (this.#keys.has(key) || this.#paths.includes(path)) return this.#mask;
    if (isMap(v)) return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, this.#scrub(`${path}.${k}`, k, x)]));
    return v;
  }

  process(rec: LogRecord): LogRecord {
    const fields = rec.fields.map(([k, v]) => [k, this.#scrub(k, k, v)] as const);
    const msg = this.#patterns.reduce((m, p) => m.replace(p, this.#mask), rec.msg);
    const same = msg === rec.msg && fields.every(([, v], i) => v === rec.fields[i]?.[1]);
    return same ? rec : changed(rec, { msg, fields });
  }

  options(): Record<string, unknown> {
    return {
      redact: {
        ...(this.#keys.size && { keys: [...this.#keys].toSorted() }),
        ...(this.#paths.length && { paths: this.#paths }),
        ...(this.#patterns.length && { patterns: this.#patterns.map((p) => p.source) }),
        ...(this.#mask !== "***" && { mask: this.#mask }),
      },
    };
  }
}

const NAMED: Readonly<Record<string, string>> = { "\n": "\\n", "\r": "\\r", "\t": "\\t" };
// biome-ignore lint/suspicious/noControlCharactersInRegex: these are what it escapes
const CONTROL = /[\x00-\x1f\x7f]/g;
const clean = (s: string): string =>
  s.replace(CONTROL, (c) => NAMED[c] ?? `\\u{${c.charCodeAt(0).toString(16).padStart(4, "0")}}`);

/** Escapes line breaks and control characters in the message and string fields, against log forging. */
export class Sanitise implements Processor {
  readonly kind = "sanitise";

  process(rec: LogRecord): LogRecord {
    const msg = clean(rec.msg);
    const fields = rec.fields.map(([k, v]) => [k, typeof v === "string" ? clean(v) : v] as const);
    const same = msg === rec.msg && fields.every(([, v], i) => v === rec.fields[i]?.[1]);
    return same ? rec : changed(rec, { msg, fields });
  }

  options(): Record<string, unknown> {
    return { sanitise: true };
  }
}

/** Sets the same fields on every record. */
export class Fields implements Processor {
  readonly kind = "fields";
  readonly #fields: Readonly<Record<string, unknown>>;

  constructor(fields: Readonly<Record<string, unknown>>) {
    this.#fields = fields;
  }

  process(rec: LogRecord): LogRecord {
    return Object.keys(this.#fields).length ? withFields(rec, Object.entries(this.#fields)) : rec;
  }

  options(): Record<string, unknown> {
    return { fields: { ...this.#fields } };
  }
}

/** Conditions of [`Match`] and [`Exclude`]. */
export interface Conditions {
  tags?: readonly string[];
  fields?: Readonly<Record<string, unknown>>;
  level?: Levelish;
}

/** Keeps a record only when every condition given holds: tags, fields shown as text, a lowest level. */
export class Match implements Processor {
  readonly kind: string = "match";
  readonly #tags: readonly string[];
  readonly #fields: readonly (readonly [string, string])[];
  readonly #level: number | undefined;

  constructor({ tags = [], fields = {}, level }: Conditions = {}) {
    this.#tags = tags;
    this.#fields = Object.entries(fields).map(([k, v]) => [k, String(v)] as const);
    this.#level = level === undefined ? undefined : parseLevel(level);
  }

  /** Whether every condition holds. */
  holds(rec: LogRecord): boolean {
    return (
      (this.#level === undefined || rec.level >= this.#level) &&
      this.#tags.every((t) => rec.tags.includes(t)) &&
      this.#fields.every(([k, v]) => field(rec, k) !== undefined && show(field(rec, k)) === v)
    );
  }

  process(rec: LogRecord): LogRecord | null {
    return this.holds(rec) ? rec : null;
  }

  options(): Record<string, unknown> {
    return {
      [this.kind]: {
        ...(this.#tags.length && { tags: this.#tags }),
        ...(this.#fields.length && { fields: Object.fromEntries(this.#fields) }),
        ...(this.#level !== undefined && { level: levelName(this.#level) }),
      },
    };
  }
}

/** Drops a record when every condition holds. */
export class Exclude extends Match {
  override readonly kind: string = "exclude";

  override process(rec: LogRecord): LogRecord | null {
    return this.holds(rec) ? null : rec;
  }
}

/** Per level and message, keeps the first `first` in each `per`, then one in `every`. */
export class Sample implements Processor {
  readonly kind = "sample";
  readonly #first: number;
  readonly #every: number;
  readonly #per: number;
  #seen = new Map<string, { at: number; n: number }>();

  constructor({ first = 100, every = 100, per = 1 }: { first?: number; every?: number; per?: number | string } = {}) {
    this.#first = first;
    this.#every = Math.max(every, 1);
    this.#per = seconds(per) * 1000;
  }

  process(rec: LogRecord): LogRecord | null {
    const now = performance.now();
    if (this.#seen.size > 10_000) this.#seen = new Map([...this.#seen].filter(([, s]) => now - s.at < this.#per));
    const slot = this.#seen.getOrInsertComputed(`${rec.level}\u0000${rec.msg}`, () => ({ at: now, n: 0 }));
    if (now - slot.at >= this.#per) Object.assign(slot, { at: now, n: 0 });
    slot.n++;
    return slot.n <= this.#first || (slot.n - this.#first) % this.#every === 0 ? rec : null;
  }

  options(): Record<string, unknown> {
    return { sample: { first: this.#first, every: this.#every, per: showTime(this.#per / 1000) } };
  }
}

/** A token bucket: `rate` records a second, up to `burst` at once. */
export class Limit implements Processor {
  readonly kind = "limit";
  readonly #rate: number;
  readonly #burst: number;
  #tokens: number;
  #last = performance.now();

  constructor({ rate, burst }: { rate: number; burst?: number }) {
    this.#rate = rate;
    this.#burst = Math.max(burst ?? rate, 1);
    this.#tokens = this.#burst;
  }

  process(rec: LogRecord): LogRecord | null {
    const now = performance.now();
    this.#tokens = Math.min(this.#burst, this.#tokens + ((now - this.#last) / 1000) * this.#rate);
    this.#last = now;
    if (this.#tokens < 1) return null;
    this.#tokens--;
    return rec;
  }

  options(): Record<string, unknown> {
    return { limit: { rate: this.#rate, burst: this.#burst } };
  }
}

/** Drops a record repeated within `within`; the next one kept carries `repeated`. */
export class Dedupe implements Processor {
  readonly kind = "dedupe";
  readonly #within: number;
  #seen = new Map<string, { at: number; n: number }>();

  constructor({ within = 1 }: { within?: number | string } = {}) {
    this.#within = seconds(within) * 1000;
  }

  process(rec: LogRecord): LogRecord | null {
    const key = `${rec.level}\u0000${rec.target}\u0000${rec.msg}`;
    const now = performance.now();
    if (this.#seen.size > 10_000) this.#seen = new Map([...this.#seen].filter(([, s]) => now - s.at < this.#within));
    const slot = this.#seen.get(key);
    if (slot && now - slot.at < this.#within) {
      slot.n++;
      return null;
    }
    this.#seen.set(key, { at: now, n: 0 });
    return slot?.n ? withFields(rec, [["repeated", slot.n]]) : rec;
  }

  options(): Record<string, unknown> {
    return { dedupe: { within: showTime(this.#within / 1000) } };
  }
}
