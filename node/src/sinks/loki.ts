/** Grafana Loki's push API, which Alloy's `loki.source.api` accepts as well. */

import { gzipSync } from "node:zlib";
import { ConfigError, Rejected } from "../errors.ts";
import { type Layout, Pattern } from "../layout/index.ts";
import { flat, quote, show } from "../layout/text.ts";
import { levelName } from "../level.ts";
import { meta } from "../meta.ts";
import { type Queue, queue } from "../queue.ts";
import type { Entry, Sink, SlotOptions } from "../sink.ts";
import { split } from "../sink.ts";
import { seconds, showTime } from "../units.ts";

/** Options of [`Loki`]. */
export interface LokiOptions extends SlotOptions {
  url: string;
  labels?: Readonly<Record<string, string>>;
  timeout?: number | string;
  connect?: boolean;
}

const object = (pairs: Iterable<readonly [string, string]>): string =>
  `{${[...pairs].map(([k, v]) => `${quote(k)}:${quote(v)}`).join(",")}}`;

/** Pushes gzipped JSON, one stream per level; lines are the slot's layout, `{msg}` unless set. */
export class Loki implements Sink {
  readonly kind = "loki";
  readonly slot: SlotOptions;
  readonly #url: string;
  readonly #labels: Readonly<Record<string, string>>;
  readonly #timeout: number;
  readonly #connect: boolean;

  constructor(options: LokiOptions) {
    const [own, slot] = split(options);
    this.slot = slot;
    this.#url = own.url;
    this.#labels = own.labels ?? {};
    this.#timeout = seconds(own.timeout ?? 10);
    this.#connect = own.connect ?? false;
  }

  /** The push request body: labels per stream, structured metadata per line. */
  body(entries: readonly Entry[]): string {
    const m = meta();
    const streams = Map.groupBy(entries, (e) => e.record.level);
    const out = [...streams.keys()]
      .toSorted((a, b) => a - b)
      .map((level) => {
        const base = (
          [
            ["app", m.app],
            ["env", m.env],
            ["instance", m.instance],
            ["level", levelName(level)],
          ] as const
        ).filter(([, v]) => v);
        const values = (streams.get(level) ?? []).map((e) => {
          const r = e.record;
          const ns = BigInt(r.time.getTime()) * 1_000_000n;
          const extra: [string, string][] = [["target", r.target]];
          if (r.file && r.line !== undefined) extra.push(["file", `${r.file}:${r.line}`]);
          for (const [k, v] of flat(r.fields)) extra.push([k, show(v)]);
          return `["${ns}",${quote(e.line)},${object(extra)}]`;
        });
        return `{"stream":${object([...base, ...Object.entries(this.#labels)])},"values":[${values.join(",")}]}`;
      });
    return `{"streams":[${out.join(",")}]}`;
  }

  emit(entry: Entry): Promise<void> {
    return this.emitBatch([entry]);
  }

  async emitBatch(entries: readonly Entry[]): Promise<void> {
    if (!entries.length) return;
    const r = await fetch(this.#url, {
      method: "POST",
      headers: { "Content-Type": "application/json", "Content-Encoding": "gzip" },
      body: gzipSync(this.body(entries)),
      signal: AbortSignal.timeout(this.#timeout * 1000),
    });
    if (r.ok) return;
    if (r.status >= 400 && r.status < 500 && r.status !== 429) throw new Rejected(`loki answered ${r.status}`);
    throw new Error(`loki answered ${r.status}`);
  }

  async ready(): Promise<void> {
    if (!/^https?:\/\//.test(this.#url)) throw new ConfigError(`loki url \`${this.#url}\` is not http or https`);
    if (!this.#connect) return;
    const i = this.#url.indexOf("/loki/api/");
    const base = i >= 0 ? this.#url.slice(0, i) : this.#url.replace(/\/+$/, "");
    const r = await fetch(`${base}/ready`, { signal: AbortSignal.timeout(this.#timeout * 1000) });
    if (!r.ok) throw new Error(`loki /ready answered ${r.status}`);
  }

  defaultLayout(): Layout {
    return new Pattern("{msg}");
  }

  queued(): Queue {
    return queue(8192);
  }

  options(): Record<string, unknown> {
    return {
      url: this.#url,
      ...(Object.keys(this.#labels).length && { labels: { ...this.#labels } }),
      ...(this.#timeout !== 10 && { timeout: showTime(this.#timeout) }),
      ...(this.#connect && { connect: true }),
    };
  }
}
