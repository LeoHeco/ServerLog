/** The layer around each sink: name, filter, processors, layout, queue, retries, health and failure policy. */

import { type Colour, parseColour, Theme, type When } from "../colour.ts";
import { ConfigError } from "../errors.ts";
import { asFilter, Filter } from "../filter.ts";
import { build, type Layout } from "../layout/index.ts";
import { parseLevel } from "../level.ts";
import { isStep, type Step } from "../process/index.ts";
import { type Queue, queue } from "../queue.ts";
import type { Entry, Sink, SlotOptions } from "../sink.ts";
import { seconds } from "../units.ts";
import type { Health } from "./breaker.ts";

/** A slot's state, for monitoring. */
export interface Status {
  readonly name: string;
  readonly kind: string;
  readonly health: Health;
  readonly skipped: number;
  readonly dropped: number;
  readonly suppressed: number;
  readonly since: Date;
  readonly error: string | undefined;
}

let makeStep: (spec: string | Readonly<Record<string, unknown>>) => Step = () => {
  throw new ConfigError("processors given by name need the config module");
};

/** Lets the config module turn processor names into steps without an import cycle. */
export function stepsFrom(make: typeof makeStep): void {
  makeStep = make;
}

/** A sink made from a function taking an entry. */
export const fromFn = (fn: (entry: Entry) => unknown, slot: SlotOptions = {}): Sink => ({
  kind: "fn",
  slot,
  emit: (e) => void fn(e),
});

/** A sink with the settings of the slot it goes into; keys are those of a config entry. */
export class Slot {
  readonly sink: Sink;
  readonly given: SlotOptions;
  readonly name: string | undefined;
  readonly filter: Filter | undefined;
  readonly process: readonly Step[];
  readonly layout: Layout | undefined;
  readonly colour: When;
  readonly theme: Theme;
  readonly alarmOnly: boolean;
  readonly onFail: unknown;
  readonly mode: Queue | "direct" | "sink";
  readonly retries: number | undefined;
  readonly probe: number;

  constructor(sink: Sink | ((entry: Entry) => unknown), options: SlotOptions = {}) {
    this.sink = typeof sink === "function" ? fromFn(sink) : sink;
    const k: SlotOptions = { ...this.sink.slot, ...options };
    this.given = k;
    this.name = k.name;
    if (k.level !== undefined && k.filter !== undefined) throw new ConfigError("give `level` or `filter`, not both");
    this.filter =
      k.level !== undefined ? new Filter(parseLevel(k.level)) : k.filter !== undefined ? asFilter(k.filter) : undefined;
    this.process = (k.process ?? []).map((p) => (isStep(p) ? p : makeStep(p)));
    this.layout = build(k);
    this.colour = k.colour ?? "auto";
    this.theme =
      k.colours instanceof Theme
        ? k.colours
        : new Theme(
            Object.entries(k.colours ?? {}).map(([l, c]) => [parseLevel(l), parseColour(c as Colour)] as const),
          );
    this.alarmOnly = k.alarmOnly ?? false;
    this.onFail = k.onFail;
    this.mode = this.#mode(k);
    this.retries = k.retries;
    this.probe = seconds(k.probe ?? 5);
  }

  #mode(k: SlotOptions): Queue | "direct" | "sink" {
    if (k.queue === undefined && k.batch === undefined && k.wait === undefined && k.full === undefined) return "sink";
    if (k.queue === 0) return "direct";
    const base = this.sink.queued?.() ?? queue(1024);
    return queue(k.queue ?? base.size, {
      batch: k.batch ?? base.batch,
      wait: k.wait ?? base.wait,
      full: k.full ?? base.full,
    });
  }
}

export { Breaker, type Health, type Phase, type Tell } from "./breaker.ts";
