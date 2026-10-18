/** The interface every output implements, built-in or not, the entry it is handed, and the slot keys it carries. */

import type { Colour, Theme, When } from "./colour.ts";
import type { Filter } from "./filter.ts";
import { INHERIT, type Layout, type LayoutKeys } from "./layout/index.ts";
import type { Levelish } from "./level.ts";
import type { Step } from "./process/index.ts";
import type { Full, Queue } from "./queue.ts";
import type { LogRecord } from "./record.ts";

/** What a sink is handed: the record, and the line its slot's layout makes of it. */
export class Entry {
  readonly record: LogRecord;
  readonly #layout: Layout;
  readonly #theme: Theme | undefined;
  readonly #cache: Map<Layout, Map<Theme | undefined, string>> | undefined;
  #line: string | undefined;

  constructor(
    record: LogRecord,
    layout: Layout = INHERIT,
    theme?: Theme,
    cache?: Map<Layout, Map<Theme | undefined, string>>,
  ) {
    this.record = record;
    this.#layout = layout;
    this.#theme = theme;
    this.#cache = cache;
  }

  /** Whether the line is coloured. */
  get coloured(): boolean {
    return this.#theme !== undefined;
  }

  /** The line, laid out on first use and shared with other slots using the same layout. */
  get line(): string {
    if (this.#line === undefined) {
      const byTheme = this.#cache?.getOrInsertComputed(this.#layout, () => new Map());
      this.#line = byTheme?.get(this.#theme) ?? this.#layout.render(this.record, this.#theme);
      byTheme?.set(this.#theme, this.#line);
    }
    return this.#line;
  }
}

/** The slot settings every sink may carry alongside its own: the same keys as a config entry, in camel case. */
export interface SlotOptions extends LayoutKeys {
  name?: string;
  level?: Levelish;
  filter?: string | Filter;
  process?: readonly (Step | string | Readonly<Record<string, unknown>>)[];
  colour?: When;
  colours?: Readonly<Record<string, Colour>> | Theme;
  alarmOnly?: boolean;
  onFail?: unknown;
  queue?: number;
  batch?: number;
  wait?: number | string;
  full?: Full;
  retries?: number;
  probe?: number | string;
}

/** The slot keys, as a list. */
export const SLOT_KEYS: ReadonlySet<string> = new Set([
  "name",
  "level",
  "filter",
  "process",
  "layout",
  "format",
  "pattern",
  "schema",
  "keys",
  "levels",
  "nest",
  "colour",
  "colours",
  "alarmOnly",
  "onFail",
  "queue",
  "batch",
  "wait",
  "full",
  "retries",
  "probe",
]);

/**
 * Somewhere entries go. Only `emit` is required; queues, retries, health, alarms and layout come from the slot, so a
 * sink only delivers and throws, or rejects, with what went wrong.
 */
export interface Sink {
  readonly kind?: string;
  readonly slot?: SlotOptions;
  emit(entry: Entry): void | Promise<void>;
  emitBatch?(entries: readonly Entry[]): void | Promise<void>;
  enabled?(level: number, target: string): boolean;
  flush?(): void | Promise<void>;
  close?(): void | Promise<void>;
  ready?(): void | Promise<void>;
  claim?(): string | undefined;
  tty?(): boolean;
  defaultLayout?(): Layout;
  queued?(): Queue | undefined;
  options?(): Record<string, unknown> | undefined;
}

/** Splits options into a sink's own and the slot's. */
export function split<T extends object>(all: T & SlotOptions): readonly [Omit<T, keyof SlotOptions>, SlotOptions] {
  const own: Record<string, unknown> = {};
  const slot: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(all)) (SLOT_KEYS.has(k) ? slot : own)[k] = v;
  return [own as Omit<T, keyof SlotOptions>, slot as SlotOptions];
}
