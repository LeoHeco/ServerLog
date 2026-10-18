/** Per-target levels written the `RUST_LOG` way: `info,app=debug,app.db=trace,hyper=off,web:*=warn/regex`. */

import { levelName, parseLevel } from "./level.ts";

interface Rule {
  readonly target: string;
  readonly glob: RegExp | undefined;
  readonly level: number | undefined;
}

const below = (target: string, rule: string): boolean => {
  if (!target.startsWith(rule)) return false;
  const rest = target.slice(rule.length);
  return rest === "" || ["::", ".", ":", "/"].some((s) => rest.startsWith(s));
};

const rank = (r: Rule): number => (r.glob ? 1e6 : 0) - r.target.replaceAll("*", "").length;

/** Which records pass, by target, level and optionally message; the most specific directive wins. */
export class Filter {
  readonly #spec: string;
  readonly #base: number | undefined;
  readonly #rules: readonly Rule[];
  readonly #pattern: RegExp | undefined;

  /**
   * Comma-separated directives; anything after the first `/` is a regex the message must match. As with
   * `env_logger`, unmatched targets are off unless a default is given, and an empty spec lets errors through.
   */
  constructor(spec: string | number = "info") {
    const text = typeof spec === "number" ? (levelName(spec) === "custom" ? String(spec) : levelName(spec)) : spec;
    this.#spec = text;
    const cut = text.indexOf("/");
    const directives = cut < 0 ? text : text.slice(0, cut);
    const off = (l: string): number | undefined => (l.toLowerCase() === "off" ? undefined : parseLevel(l));
    let base: number | undefined = directives.trim() ? undefined : 50;
    const rules: Rule[] = [];
    for (const d of directives
      .split(",")
      .map((x) => x.trim())
      .filter(Boolean)) {
      const eq = d.indexOf("=");
      if (eq >= 0) {
        rules.push(this.#rule(d.slice(0, eq).trim(), off(d.slice(eq + 1).trim())));
        continue;
      }
      try {
        base = off(d);
      } catch {
        rules.push(this.#rule(d, 0));
      }
    }
    this.#base = base;
    this.#rules = rules.toSorted((a, b) => rank(a) - rank(b));
    this.#pattern = cut < 0 ? undefined : new RegExp(text.slice(cut + 1));
  }

  #rule(target: string, level: number | undefined): Rule {
    const glob = target.includes("*") ? new RegExp(`^${target.split("*").map(RegExp.escape).join(".*")}$`) : undefined;
    return { target, glob, level };
  }

  /** Whether a record at `level` from `target` passes, before its message is looked at. */
  allows(level: number, target: string): boolean {
    const rule = this.#rules.find((r) => (r.glob ? r.glob.test(target) : below(target, r.target)));
    const min = rule ? rule.level : this.#base;
    return min !== undefined && level >= min;
  }

  /** Whether the message passes the `/regex` part, if there is one. */
  admits(msg: string): boolean {
    return this.#pattern?.test(msg) ?? true;
  }

  /** Whether there is a `/regex` part. */
  get hasPattern(): boolean {
    return this.#pattern !== undefined;
  }

  /** The lowest level any target lets through, or `undefined` when everything is off. */
  get floor(): number | undefined {
    const all = [this.#base, ...this.#rules.map((r) => r.level)].filter((l) => l !== undefined);
    return all.length ? Math.min(...all) : undefined;
  }

  toString(): string {
    return this.#spec;
  }
}

/** A filter from a spec or a level. */
export const asFilter = (f: Filter | string | number): Filter => (f instanceof Filter ? f : new Filter(f));
