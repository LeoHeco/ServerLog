/** Standard output or standard error, one line per entry. */

import { writeSync } from "node:fs";
import { compat, type Stream, streamOf } from "../compat.ts";
import { consoleFrom } from "../hub.ts";
import type { Entry, Sink, SlotOptions } from "../sink.ts";
import { split } from "../sink.ts";

/** Options of [`Console`]. */
export interface ConsoleOptions extends SlotOptions {
  /** `stdout`, `stderr` or `auto`; the profile's choice unless given. */
  stream?: Stream;
}

/** Prints one line per entry. */
export class Console implements Sink {
  readonly kind = "console";
  readonly slot: SlotOptions;
  readonly #stream: Stream | undefined;

  constructor(options: ConsoleOptions = {}) {
    const [own, slot] = split(options);
    this.slot = slot;
    this.#stream = own.stream;
  }

  #fd(level: number): 1 | 2 {
    const s = this.#stream ?? streamOf(compat());
    return s === "stderr" || (s === "auto" && level >= 50) ? 2 : 1;
  }

  emit(entry: Entry): void {
    writeSync(this.#fd(entry.record.level), `${entry.line}\n`);
  }

  emitBatch(entries: readonly Entry[]): void {
    for (const fd of [1, 2] as const) {
      const text = entries
        .filter((e) => this.#fd(e.record.level) === fd)
        .map((e) => `${e.line}\n`)
        .join("");
      if (text) writeSync(fd, text);
    }
  }

  tty(): boolean {
    return Boolean((this.#fd(30) === 2 ? process.stderr : process.stdout).isTTY);
  }

  options(): Record<string, unknown> {
    return this.#stream ? { stream: this.#stream } : {};
  }
}

consoleFrom((stream) => new Console(stream ? { stream } : {}));
