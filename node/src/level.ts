/** Severity: six built-in levels from 10 to 60, plus any defined at run time; levels are plain numbers. */

import type { Colour } from "./colour.ts";
import { type Compat, compat } from "./compat.ts";

/** The built-in levels. */
export const Level = { TRACE: 10, DEBUG: 20, INFO: 30, WARN: 40, ERROR: 50, FATAL: 60 } as const;

/** A level: a number from 0 to 255, or a name such as `"warn"`. */
export type Levelish = number | string;

const BUILT_IN = new Map<number, readonly [string, Colour]>([
  [10, ["trace", "cyan"]],
  [20, ["debug", "magenta"]],
  [30, ["info", "green"]],
  [40, ["warn", "yellow"]],
  [50, ["error", "red"]],
  [60, ["fatal", "alarm"]],
]);
const ALIASES: Readonly<Record<string, number>> = { warning: 40, critical: 60 };
const custom = new Map<number, readonly [string, Colour]>();

const entry = (v: number): readonly [string, Colour] | undefined => BUILT_IN.get(v) ?? custom.get(v);

/** A level's number, from a number or a name in any case. */
export function parseLevel(level: Levelish): number {
  if (typeof level === "number") {
    if (!Number.isInteger(level) || level < 0 || level > 255) throw new RangeError(`a level is 0 to 255, not ${level}`);
    return level;
  }
  const s = level.trim().toLowerCase();
  if (/^\d+$/.test(s)) return parseLevel(Number(s));
  const found = ALIASES[s] ?? [...BUILT_IN, ...custom].find(([, [n]]) => n === s)?.[0];
  if (found === undefined) {
    throw new TypeError(
      `unknown level \`${level}\`: expected trace, debug, info, warn, error, fatal, a defined level or a number`,
    );
  }
  return found;
}

/** Defines a level, or renames the one with that number. */
export function defineLevel(name: string, value: number, colour: Colour = "plain"): number {
  if (
    !/^[a-z0-9_]+$/.test(name) ||
    name === "off" ||
    name in ALIASES ||
    [...BUILT_IN.values()].some(([n]) => n === name)
  ) {
    throw new TypeError(`bad level name \`${name}\``);
  }
  for (const [v, [n]] of custom) if (v === value || n === name) custom.delete(v);
  custom.set(parseLevel(value), [name, colour]);
  return value;
}

/** Every level defined at run time. */
export const definedLevels = (): ReadonlyMap<number, readonly [string, Colour]> => custom;

/** The lower-case name used in config files, filters and labels. */
export const levelName = (v: number): string => entry(v)?.[0] ?? "custom";

/** Its colour in a terminal. */
export const levelColour = (v: number): Colour => entry(v)?.[1] ?? "plain";

/** How a profile prints a level, such as `WARNING` for Python's. */
export function levelLabel(v: number, profile: Compat = compat()): string {
  if (profile === "python" && v === 40) return "WARNING";
  if (profile === "python" && v === 60) return "CRITICAL";
  if (profile === "slog" && v === 10) return "DEBUG-4";
  if (profile === "slog" && v === 60) return "ERROR+4";
  return levelName(v).toUpperCase();
}

/** The syslog severity, for GELF. */
export const syslog = (v: number): number => (v <= 20 ? 7 : v <= 30 ? 6 : v <= 40 ? 4 : v <= 50 ? 3 : 2);
