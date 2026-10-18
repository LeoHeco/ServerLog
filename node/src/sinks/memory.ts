/** A ring buffer in memory, for tests and for looking back at recent records. */

import type { LogRecord } from "../record.ts";
import type { Entry, Sink, SlotOptions } from "../sink.ts";
import { split } from "../sink.ts";

/** Options of [`Memory`]. */
export interface MemoryOptions extends SlotOptions {
  /** How many records to keep; the oldest go first. */
  size?: number;
}

/** Keeps the newest records and their lines. */
export class Memory implements Sink {
  readonly kind = "memory";
  readonly slot: SlotOptions;
  readonly #size: number;
  #kept: (readonly [LogRecord, string])[] = [];

  constructor(options: MemoryOptions = {}) {
    const [own, slot] = split(options);
    this.slot = slot;
    this.#size = Math.max(own.size ?? 1000, 1);
  }

  emit(entry: Entry): void {
    this.#kept.push([entry.record, entry.line]);
    if (this.#kept.length > this.#size) this.#kept.shift();
  }

  /** The records kept, oldest first. */
  get records(): LogRecord[] {
    return this.#kept.map(([r]) => r);
  }

  /** The lines kept, oldest first. */
  get lines(): string[] {
    return this.#kept.map(([, l]) => l);
  }

  /** The records for which `test` holds. */
  find(test: (rec: LogRecord) => boolean): LogRecord[] {
    return this.records.filter(test);
  }

  /** Forgets everything. */
  clear(): void {
    this.#kept = [];
  }

  /** How many are kept. */
  get length(): number {
    return this.#kept.length;
  }

  options(): Record<string, unknown> {
    return { size: this.#size };
  }
}
