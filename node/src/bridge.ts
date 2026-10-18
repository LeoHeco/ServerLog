/** `console` and the `DEBUG` variable of the `debug` package, routed into serlog. */

import { format } from "node:util";
import { emit, enabled } from "./hub.ts";
import { Level } from "./level.ts";
import { make } from "./logger.ts";

const METHODS = {
  trace: Level.TRACE,
  debug: Level.DEBUG,
  log: Level.INFO,
  info: Level.INFO,
  warn: Level.WARN,
  error: Level.ERROR,
} as const;
type Method = keyof typeof METHODS;

/** Filter directives for a `DEBUG` value: `app:*,-app:noisy` lets `app:*` through at debug and turns `app:noisy` off. */
export const debugDirectives = (spec: string): string =>
  spec
    .split(/[\s,]+/)
    .filter(Boolean)
    .map((ns) => (ns.startsWith("-") ? `${ns.slice(1)}=off` : `${ns}=debug`))
    .join(",");

/** Sends `console.log` and its kin into serlog under the target `console`, until disposed: `using _ = capture()`. */
export function capture(): Disposable {
  const saved = (Object.keys(METHODS) as Method[]).map((m) => [m, console[m]] as const);
  for (const [m, level] of Object.entries(METHODS) as [Method, number][]) {
    console[m] = (...args: unknown[]) => {
      if (enabled(level)) emit(make(level, "console", [], [format(...args)]));
    };
  }
  return {
    [Symbol.dispose]() {
      for (const [m, fn] of saved) console[m] = fn;
    },
  };
}
