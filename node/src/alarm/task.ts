/** The alarm route: one queue and one task, both made when the first incident arrives. */

import { adopt } from "../guard.ts";
import { type Alarm, type Desk, type Incident, resolve } from "./index.ts";
import { sound } from "./walk.ts";

const LIMIT = 64;
const jobs: (readonly [Alarm, Incident])[] = [];
let running: Promise<void> | undefined;
let lost = 0;
let desk: (() => Desk) | undefined;
let flushSlots: ((within: number) => Promise<void>) | undefined;

/** Lets the hub supply its desk and flush without an import cycle. */
export function wire(d: () => Desk, flush: (within: number) => Promise<void>): void {
  desk = d;
  flushSlots = flush;
}

async function drain(): Promise<void> {
  for (let job = jobs.shift(); job; job = jobs.shift()) {
    const [alarm, incident] = job;
    if (desk && (await sound(alarm, incident, desk())) === "unheard" && alarm.fallback === "panic") {
      await crash(`no sink took the alarm for \`${incident.sink}\`: ${incident.error}`);
    }
  }
  running = undefined;
}

/** Queues `incident` for `alarm`; never blocks, and counts what a full queue drops. */
export function post(alarm: Alarm, incident: Incident): void {
  if (jobs.length >= LIMIT) {
    lost++;
    return;
  }
  jobs.push([resolve(alarm), incident]);
  running ??= adopt(null, () => Promise.resolve().then(drain));
}

/** Waits up to `within` seconds for the incidents queued so far. */
export async function settle(within: number): Promise<void> {
  if (!running) return;
  await Promise.race([running, new Promise((r) => setTimeout(r, within * 1000).unref())]);
}

/** Incidents a full alarm queue dropped. */
export const dropped = (): number => lost;

/** Says why on standard error, gives the other sinks a second to flush, and exits with 70. */
export async function crash(why: string): Promise<never> {
  try {
    process.stderr.write(`serlog: ${why}; exiting\n`);
  } catch {}
  await flushSlots?.(1);
  process.exit(70);
}
