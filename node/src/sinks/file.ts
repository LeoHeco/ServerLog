/** A log file, rolled over by size or time, cleaned up by count, age or total size, and reopened when moved. */

import {
  closeSync,
  createReadStream,
  createWriteStream,
  existsSync,
  fstatSync,
  mkdirSync,
  openSync,
  readdirSync,
  realpathSync,
  renameSync,
  statSync,
  unlinkSync,
  utimesSync,
  writeSync,
} from "node:fs";
import { basename, dirname, extname, join, resolve } from "node:path";
import { pipeline } from "node:stream/promises";
import { createGzip } from "node:zlib";
import { ConfigError } from "../errors.ts";
import { strftime } from "../layout/clock.ts";
import type { Entry, Sink, SlotOptions } from "../sink.ts";
import { split } from "../sink.ts";
import { seconds, showSize, showTime, size } from "../units.ts";

/** When files roll over by time, on local time. */
export type Rotate = "minute" | "hour" | "day" | "week";

const PERIODS: Readonly<Record<Rotate, string>> = {
  minute: "%Y%m%d%H%M",
  hour: "%Y%m%d%H",
  day: "%Y%m%d",
  week: "%G%V",
};

/** The default name of a rolled file. */
export const NAME = "{stem}.{time:%Y%m%d-%H%M%S}.{ext}";

/** Options of [`File`]. */
export interface FileOptions extends SlotOptions {
  path: string;
  append?: boolean;
  perms?: string | number;
  mkdir?: boolean;
  maxSize?: number | string;
  rotate?: Rotate;
  keep?: number;
  maxAge?: number | string;
  maxTotal?: number | string;
  gzip?: boolean;
  rollName?: string;
  reopen?: boolean;
  buffer?: number | string;
  flushEvery?: number | string;
}

const glob = (pattern: string, name: string): boolean =>
  new RegExp(`^${pattern.split("*").map(RegExp.escape).join(".*")}$`).test(name);

/** Appends one line per entry to `path`. */
export class File implements Sink {
  readonly kind = "file";
  readonly slot: SlotOptions;
  readonly #path: string;
  readonly #o: Required<Pick<FileOptions, "append" | "mkdir" | "gzip" | "reopen">> & { rollName: string };
  readonly #perms: number | undefined;
  readonly #max: number | undefined;
  readonly #rotate: Rotate | undefined;
  readonly #keep: number | undefined;
  readonly #age: number | undefined;
  readonly #total: number | undefined;
  readonly #buffer: number;
  readonly #every: number;
  #fd: number | undefined;
  #size = 0;
  #period: string | undefined;
  #began = new Date();
  #checked = 0;
  #flushed: number | undefined;
  #pending = "";
  #jobs: Promise<void> = Promise.resolve();

  constructor(options: FileOptions) {
    const [own, slot] = split(options);
    this.slot = slot;
    this.#path = own.path;
    this.#o = {
      append: own.append ?? true,
      mkdir: own.mkdir ?? true,
      gzip: own.gzip ?? false,
      reopen: own.reopen ?? false,
      rollName: own.rollName ?? NAME,
    };
    this.#perms = typeof own.perms === "string" ? Number.parseInt(own.perms, 8) : own.perms;
    this.#max = own.maxSize === undefined ? undefined : size(own.maxSize);
    this.#rotate = own.rotate;
    this.#keep = own.keep;
    this.#age = own.maxAge === undefined ? undefined : seconds(own.maxAge);
    this.#total = own.maxTotal === undefined ? undefined : size(own.maxTotal);
    this.#buffer = size(own.buffer ?? 0);
    this.#every = seconds(own.flushEvery ?? 1);
  }

  /** The path written to. */
  get path(): string {
    return this.#path;
  }

  #open(fresh: boolean): void {
    const isNew = !existsSync(this.#path);
    this.#fd = openSync(this.#path, fresh && !this.#o.append ? "w" : "a", this.#perms ?? 0o644);
    const st = fstatSync(this.#fd);
    this.#size = isNew ? 0 : st.size;
    this.#began = st.mtime;
    this.#period = this.#rotate && strftime(this.#began, PERIODS[this.#rotate]);
  }

  #name(began: Date): string {
    const ext = extname(this.#path).slice(1);
    const stem = basename(this.#path, extname(this.#path));
    const out = this.#o.rollName
      .replace(/\{([^}]*)\}/g, (all, hole: string) =>
        hole === "stem" ? stem : hole === "ext" ? ext : hole.startsWith("time:") ? strftime(began, hole.slice(5)) : all,
      )
      .replace(/\.+$/, "");
    const free = (n: string): boolean =>
      !existsSync(join(dirname(this.#path), n)) && !existsSync(join(dirname(this.#path), `${n}.gz`));
    if (free(out)) return join(dirname(this.#path), out);
    const dot = out.lastIndexOf(".");
    const [base, tail] = dot > 0 ? [out.slice(0, dot), out.slice(dot)] : [out, ""];
    for (let n = 1; ; n++) if (free(`${base}-${n}${tail}`)) return join(dirname(this.#path), `${base}-${n}${tail}`);
  }

  #shape(): readonly [string, string] {
    const ext = extname(this.#path).slice(1);
    const stem = basename(this.#path, extname(this.#path));
    const plain = this.#o.rollName
      .replace(/\{([^}]*)\}/g, (_, h: string) => (h === "stem" ? stem : h === "ext" ? ext : "*"))
      .replace(/\.+$/, "");
    const dot = plain.lastIndexOf(".");
    return [plain, dot > 0 ? `${plain.slice(0, dot)}*${plain.slice(dot)}` : `${plain}*`];
  }

  #prune(): void {
    if (this.#keep === undefined && this.#age === undefined && this.#total === undefined) return;
    const [plain, numbered] = this.#shape();
    const dir = dirname(this.#path);
    const old = readdirSync(dir)
      .filter((n) => n !== basename(this.#path) && [plain, numbered].some((p) => glob(p, n.replace(/\.gz$/, ""))))
      .map((n) => {
        const st = statSync(join(dir, n));
        return { p: join(dir, n), t: st.mtimeMs, n: st.size };
      })
      .toSorted((a, b) => a.t - b.t || a.p.localeCompare(b.p));
    let total = old.reduce((s, x) => s + x.n, 0);
    let left = old.length;
    for (const f of old) {
      const aged = this.#age !== undefined && Date.now() - f.t > this.#age * 1000;
      const many = this.#keep !== undefined && left > this.#keep;
      const big = this.#total !== undefined && total > this.#total;
      if (!(aged || many || big)) continue;
      unlinkSync(f.p);
      left--;
      total -= f.n;
    }
  }

  async #after(rolled: string): Promise<void> {
    if (this.#o.gzip) {
      const mtime = statSync(rolled).mtime;
      await pipeline(createReadStream(rolled), createGzip(), createWriteStream(`${rolled}.gz`));
      utimesSync(`${rolled}.gz`, mtime, mtime);
      unlinkSync(rolled);
    }
    this.#prune();
  }

  #roll(t: Date): void {
    this.#drain();
    if (this.#fd !== undefined) closeSync(this.#fd);
    const to = this.#name(this.#began);
    renameSync(this.#path, to);
    this.#open(false);
    this.#began = t;
    this.#period = this.#rotate && strftime(t, PERIODS[this.#rotate]);
    this.#jobs = this.#jobs.then(() => this.#after(to));
  }

  #drain(): void {
    if (this.#pending && this.#fd !== undefined) {
      writeSync(this.#fd, this.#pending);
      this.#pending = "";
    }
    this.#flushed = performance.now();
  }

  #put(text: string, t: Date, urgent: boolean): void {
    const now = performance.now();
    if (this.#o.reopen && this.#fd !== undefined && now - this.#checked >= 1000) {
      this.#checked = now;
      if (!existsSync(this.#path) || statSync(this.#path).size < this.#size) {
        this.#drain();
        closeSync(this.#fd);
        this.#fd = undefined;
      }
    }
    if (this.#fd === undefined) this.#open(false);
    const n = Buffer.byteLength(text);
    const full = this.#max !== undefined && this.#size > 0 && this.#size + n > this.#max;
    const turned = this.#rotate !== undefined && this.#period !== strftime(t, PERIODS[this.#rotate]);
    if (full || turned) this.#roll(t);
    this.#size += n;
    if (!this.#buffer) {
      writeSync(this.#fd as number, text);
      return;
    }
    this.#pending += text;
    if (
      urgent ||
      this.#pending.length >= this.#buffer ||
      this.#flushed === undefined ||
      now - this.#flushed >= this.#every * 1000
    )
      this.#drain();
  }

  emit(entry: Entry): void {
    this.#put(`${entry.line}\n`, entry.record.time, entry.record.level >= 50);
  }

  emitBatch(entries: readonly Entry[]): void {
    const last = entries.at(-1);
    if (!last) return;
    if (this.#max !== undefined || this.#rotate !== undefined) {
      for (const e of entries) this.emit(e);
      return;
    }
    this.#put(
      entries.map((e) => `${e.line}\n`).join(""),
      last.record.time,
      entries.some((e) => e.record.level >= 50),
    );
  }

  async flush(): Promise<void> {
    this.#drain();
    await this.#jobs;
  }

  async close(): Promise<void> {
    await this.flush();
    if (this.#fd !== undefined) closeSync(this.#fd);
    this.#fd = undefined;
  }

  ready(): void {
    if (existsSync(this.#path) && statSync(this.#path).isDirectory())
      throw new ConfigError(`${this.#path} is a directory`);
    const dir = dirname(this.#path);
    if (this.#o.mkdir) mkdirSync(dir, { recursive: true });
    else if (!existsSync(dir)) throw new ConfigError(`${dir} does not exist`);
    this.#open(true);
  }

  claim(): string {
    let p: string;
    try {
      p = realpathSync(this.#path);
    } catch {
      p = resolve(this.#path);
    }
    return `file ${process.platform === "win32" ? p.toLowerCase() : p}`;
  }

  options(): Record<string, unknown> {
    return {
      path: this.#path,
      ...(!this.#o.append && { append: false }),
      ...(this.#perms !== undefined && { perms: this.#perms.toString(8).padStart(4, "0") }),
      ...(!this.#o.mkdir && { mkdir: false }),
      ...(this.#max !== undefined && { max_size: showSize(this.#max) }),
      ...(this.#rotate && { rotate: this.#rotate }),
      ...(this.#keep !== undefined && { keep: this.#keep }),
      ...(this.#age !== undefined && { max_age: showTime(this.#age) }),
      ...(this.#total !== undefined && { max_total: showSize(this.#total) }),
      ...(this.#o.gzip && { gzip: true }),
      ...(this.#o.rollName !== NAME && { roll_name: this.#o.rollName }),
      ...(this.#o.reopen && { reopen: true }),
      ...(this.#buffer && { buffer: showSize(this.#buffer), flush_every: showTime(this.#every) }),
    };
  }
}
