/** Durations and sizes as config files write them: `250ms`, `5s`, `2m`, `1h`, `1d`; `500KB`, `64MiB`. */

const TIME: Readonly<Record<string, number>> = { ms: 0.001, "": 1, s: 1, m: 60, h: 3600, d: 86400 };
const SIZE: Readonly<Record<string, number>> = {
  "": 1,
  b: 1,
  k: 1e3,
  kb: 1e3,
  kib: 1024,
  m: 1e6,
  mb: 1e6,
  mib: 1024 ** 2,
  g: 1e9,
  gb: 1e9,
  gib: 1024 ** 3,
};
const NUMBER = /^\s*(\d+(?:\.\d+)?)\s*([a-z]*)\s*$/i;

/** A duration in seconds: a number, or text with `ms`, `s`, `m`, `h` or `d`. */
export function seconds(v: number | string): number {
  if (typeof v === "number") {
    if (v < 0) throw new RangeError(`a duration cannot be negative: ${v}`);
    return v;
  }
  const m = NUMBER.exec(v);
  const unit = TIME[m?.[2] ?? "?"];
  if (!m || unit === undefined) throw new TypeError(`bad duration \`${v}\``);
  return Number(m[1]) * unit;
}

/** A byte count: a number, or text such as `500KB` or `64MiB`. */
export function size(v: number | string): number {
  if (typeof v === "number") return v;
  const m = NUMBER.exec(v);
  const unit = SIZE[m?.[2]?.toLowerCase() ?? "?"];
  if (!m || unit === undefined) throw new TypeError(`bad size \`${v}\``);
  return Math.floor(Number(m[1]) * unit);
}

/** The shortest way to write a duration that reads back the same. */
export function showTime(secs: number): string {
  const ms = Math.round(secs * 1000);
  for (const [n, u] of [
    [86_400_000, "d"],
    [3_600_000, "h"],
    [60_000, "m"],
    [1000, "s"],
  ] as const) {
    if (ms >= n && ms % n === 0) return `${ms / n}${u}`;
  }
  return `${ms}ms`;
}

/** The shortest way to write a byte count that reads back the same. */
export function showSize(n: number): string | number {
  for (const [m, u] of [
    [1024 ** 3, "GiB"],
    [1024 ** 2, "MiB"],
    [1024, "KiB"],
  ] as const) {
    if (n >= m && n % m === 0) return `${n / m}${u}`;
  }
  return n;
}
