/** The shape of a queue in front of a sink. */

import { seconds } from "./units.ts";

/** What a full queue does with a new record. */
export type Full = "drop" | "block";

/** Holds `size` records, written `batch` at a time or `wait` seconds after the first arrives. */
export interface Queue {
  readonly size: number;
  readonly batch: number;
  readonly wait: number;
  readonly full: Full;
}

/** A queue shape with defaults: 1000 a batch, one second, drop when full. */
export const queue = (size: number, more: { batch?: number; wait?: number | string; full?: Full } = {}): Queue => ({
  size,
  batch: more.batch ?? 1000,
  wait: seconds(more.wait ?? 1),
  full: more.full ?? "drop",
});
