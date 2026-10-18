/** Line templates: `{time} {level:<5} {target}: {msg}{fields}`, with width, alignment and truncation on any part. */

import { basename } from "node:path";
import { paint, type Theme } from "../colour.ts";
import { type Compat, compat, PATTERNS, parseCompat } from "../compat.ts";
import { levelColour, levelLabel, levelName } from "../level.ts";
import { hostName, meta } from "../meta.ts";
import { field, type LogRecord } from "../record.ts";
import { elapsed, writeTime } from "./clock.ts";
import { fieldText, logfmt, show } from "./text.ts";

interface Piece {
  readonly kind: string;
  readonly option: string | number | boolean;
  readonly align: string | undefined;
  readonly width: number;
  readonly cut: number | undefined;
}

const PLAIN = new Set([
  "module",
  "func",
  "line",
  "thread",
  "tid",
  "task",
  "pid",
  "host",
  "app",
  "env",
  "instance",
  "tags",
  "elapsed",
  "seq",
  "spans",
]);
const OPTIONS: Readonly<Record<string, string>> = {
  level: "lower",
  file: "base",
  msg: "q",
  fields: "q",
  error: "stack",
};
const FIT = /^(?:([<>^])(\d+))?(?:\.(\d+))?$/;

/** Shortens leading segments of `target` to their first letter until it fits in `n`. */
export function shorten(target: string, n: number): string {
  if (target.length <= n) return target;
  const sep = target.includes("::") ? "::" : ".";
  const parts = target.split(sep);
  const out = [...parts];
  for (let i = 0; i < parts.length - 1 && out.join(sep).length > n; i++) out[i] = parts[i]?.slice(0, 1) ?? "";
  return out.join(sep);
}

function align(text: string, how: string | undefined, width: number): string {
  if (how === "<") return text.padEnd(width);
  if (how === ">") return text.padStart(width);
  if (how === "^") {
    const pad = Math.max(width - text.length, 0);
    return " ".repeat(Math.floor(pad / 2)) + text + " ".repeat(pad - Math.floor(pad / 2));
  }
  return text;
}

/**
 * A parsed line template: `{token}` or `{token:spec}` holes; `{{` and `}}` are literal braces. Every token but `time`
 * may end with `<N`, `>N` or `^N` to pad, then `.M` to cut. A preset names its levels the way its logger does.
 */
export class Pattern {
  readonly #source: string;
  readonly #pieces: readonly Piece[];
  readonly #showsError: boolean;
  readonly #labels: Compat | undefined;

  /** Parses a pattern, or a preset name such as `tracing`. */
  constructor(source: string) {
    const named = source === "log4j" ? "logback" : source;
    const s = PATTERNS[named] ?? source;
    this.#source = source;
    this.#labels = named in PATTERNS ? parseCompat(named) : undefined;
    const pieces: Piece[] = [];
    let lit = "";
    let at = 0;
    for (const m of s.matchAll(/\{\{|\}\}|\{([^}]*)\}|\}/g)) {
      lit += s.slice(at, m.index);
      at = m.index + m[0].length;
      if (m[0] === "{{" || m[0] === "}}") {
        lit += m[0][0];
        continue;
      }
      if (m[0] === "}") throw new TypeError(`pattern \`${s}\`: stray \`}\``);
      if (lit) pieces.push({ kind: "", option: lit, align: undefined, width: 0, cut: undefined });
      lit = "";
      const hole = m[1] ?? "";
      const colon = hole.indexOf(":");
      pieces.push(
        this.#piece(colon < 0 ? hole.trim() : hole.slice(0, colon).trim(), colon < 0 ? "" : hole.slice(colon + 1), s),
      );
    }
    if (s.slice(at).includes("{")) throw new TypeError(`pattern \`${s}\`: unclosed \`{\``);
    lit += s.slice(at);
    if (lit) pieces.push({ kind: "", option: lit, align: undefined, width: 0, cut: undefined });
    this.#pieces = pieces;
    this.#showsError = pieces.some((p) => p.kind === "error");
  }

  #piece(name: string, spec: string, s: string): Piece {
    const bad = (why: string): TypeError => new TypeError(`pattern \`${s}\`: ${why}`);
    if (name === "time") return { kind: "time", option: spec, align: undefined, width: 0, cut: undefined };
    let option: string | number | boolean = false;
    let rest = spec;
    const word = OPTIONS[name];
    if (name === "target" && spec.startsWith("~")) {
      const m = /^~(\d+)/.exec(spec);
      if (!m) throw bad(`bad \`~\` in \`${spec}\``);
      option = Number(m[1]);
      rest = spec.slice(m[0].length);
    } else if (word !== undefined) {
      option = spec.startsWith(word);
      rest = option ? spec.slice(word.length) : spec;
    } else if (!PLAIN.has(name) && name !== "target") {
      throw bad(`unknown token \`${name}\``);
    }
    const m = FIT.exec(rest);
    if (!m) throw bad(`bad spec \`${spec}\` for \`${name}\``);
    return {
      kind: name,
      option,
      align: m[1],
      width: Number(m[2] ?? 0),
      cut: m[3] === undefined ? undefined : Number(m[3]),
    };
  }

  /** The text it was parsed from, a preset name included. */
  get source(): string {
    return this.#source;
  }

  /** `rec` as one line; a theme paints the level. */
  render(rec: LogRecord, theme?: Theme): string {
    let out = "";
    for (const p of this.#pieces) {
      if (!p.kind) {
        out += String(p.option);
        continue;
      }
      let text = this.#token(p, rec);
      if (p.cut !== undefined) text = [...text].slice(0, p.cut).join("");
      text = align(text, p.align, p.width);
      if (p.kind === "level" && theme) text = paint(true, theme.of(rec.level, levelColour(rec.level)), text);
      out += text;
    }
    return out;
  }

  #token(p: Piece, rec: LogRecord): string {
    switch (p.kind) {
      case "time":
        return writeTime(String(p.option), rec.time);
      case "level":
        return p.option ? levelName(rec.level) : levelLabel(rec.level, this.#labels ?? compat());
      case "target":
        return typeof p.option === "number" ? shorten(rec.target, p.option) : rec.target;
      case "module":
        return rec.module ?? rec.target;
      case "func":
        return rec.func ?? "?";
      case "file":
        return p.option && rec.file ? basename(rec.file) : (rec.file ?? "?");
      case "line":
        return rec.line === undefined ? "?" : String(rec.line);
      case "thread":
        return rec.thread ?? "?";
      case "tid":
        return String(process.pid);
      case "task":
        return "?";
      case "pid":
        return String(process.pid);
      case "host":
        return hostName();
      case "app":
      case "env":
      case "instance":
        return meta()[p.kind];
      case "msg":
        return p.option ? logfmt(rec.msg) : rec.msg;
      case "fields":
        return fieldText(rec, Boolean(p.option), this.#showsError ? ["error"] : []);
      case "tags":
        return rec.tags.join(",");
      case "error": {
        const e = field(rec, "error");
        if (e === undefined) return "";
        return show(e) + (p.option && Error.isError(e) && e.stack ? `\n${e.stack}` : "");
      }
      case "elapsed":
        return String(elapsed());
      case "seq":
        return String(rec.seq);
      case "spans":
        return rec.spans ? `${rec.spans}: ` : "";
      default:
        return "";
    }
  }

  toString(): string {
    return this.#source;
  }
}
