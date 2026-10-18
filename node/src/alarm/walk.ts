/** Walking an alarm's tiers: the same rules in every language, checked by `spec/cases/alarm.json`. */

import { message } from "../errors.ts";
import type { LogRecord } from "../record.ts";
import type { Alarm, Incident } from "./index.ts";

/** What the walk needs from the sinks around it. */
export interface Desk {
  logs(): string[];
  resting(name: string): boolean;
  deliver(name: string, rec: LogRecord): void | Promise<void>;
}

/** How a walk ended. */
export type Outcome = "heard" | "unheard";

/** Delivers `incident` through `alarm`'s tiers, adding every failed delivery to it. */
export async function sound(alarm: Alarm, incident: Incident, desk: Desk): Promise<Outcome> {
  const out = new Set([incident.sink]);
  for (const tier of alarm.route(incident)) {
    const names = new Set<string>();
    for (const n of tier)
      for (const m of n === "*" ? desk.logs() : [n]) if (!out.has(m) && !desk.resting(m)) names.add(m);
    if (!names.size) continue;
    const rec = alarm.describe(incident);
    const results = await Promise.allSettled([...names].map(async (n) => desk.deliver(n, rec)));
    let heard = false;
    for (const [i, name] of [...names].entries()) {
      const r = results[i];
      if (r?.status === "fulfilled") heard = true;
      else incident.failures.push({ sink: name, error: message(r?.reason), time: new Date() });
      out.add(name);
    }
    if (heard) return "heard";
  }
  return "unheard";
}
