/** A registered slot: the core both the caller and the queue use, and the queue in front of it. */

import { Alarm, crash, DEFAULT, incident, post, resolve } from "../alarm/index.ts";
import type { Theme } from "../colour.ts";
import { resolveWhen } from "../colour.ts";
import { message, SerlogError, transient } from "../errors.ts";
import { adopt, inside } from "../guard.ts";
import { INHERIT, type Layout } from "../layout/index.ts";
import { run, type Step } from "../process/index.ts";
import type { Queue } from "../queue.ts";
import type { LogRecord } from "../record.ts";
import { Entry, type Sink } from "../sink.ts";
import { Breaker, type Phase, type Slot, type Status, type Tell } from "./index.ts";

/** What a slot does when its sink fails. */
export type Policy = "panic" | "ignore" | Alarm;

/** The failure policy an `onFail` setting names. */
export function policy(onFail: unknown, named: ReadonlyMap<string, Alarm>): Policy {
  if (onFail === undefined || onFail === null || onFail === "default") return DEFAULT;
  if (onFail === "panic" || onFail === "ignore" || onFail instanceof Alarm) return onFail;
  const found = typeof onFail === "string" ? named.get(onFail) : undefined;
  if (!found) throw new SerlogError(`\`on_fail\` names \`${String(onFail)}\`, which is not in \`alarms\``);
  return found;
}

const now = (): number => performance.now() / 1000;
const sleep = (secs: number): Promise<void> => new Promise((r) => setTimeout(r, secs * 1000).unref());
const isPromise = (x: unknown): x is Promise<unknown> => x instanceof Promise;
type Cache = Map<Layout, Map<Theme | undefined, string>>;

/** The sink, its look, its policy and its health. */
export class Core {
  readonly name: string;
  readonly kind: string;
  readonly sink: Sink;
  readonly layout: Layout;
  readonly theme: Theme | undefined;
  readonly onFail: Policy;
  readonly retries: number;
  readonly breaker: Breaker;
  dropped = 0;
  suppressed = 0;

  constructor(name: string, slot: Slot, onFail: Policy, retries: number) {
    this.name = name;
    this.sink = slot.sink;
    this.kind = slot.sink.kind ?? "custom";
    this.layout = slot.layout ?? slot.sink.defaultLayout?.() ?? INHERIT;
    this.theme = resolveWhen(slot.colour, slot.sink.tty?.() ?? false) ? slot.theme : undefined;
    this.onFail = onFail;
    this.retries = retries;
    this.breaker = new Breaker(slot.probe, now());
  }

  get up(): boolean {
    return this.breaker.health === "up";
  }

  get tell(): Tell {
    return this.onFail instanceof Alarm ? "report" : "silent";
  }

  entry(rec: LogRecord, cache?: Cache): Entry {
    return new Entry(rec, this.layout, this.theme, cache);
  }

  #batch(entries: readonly Entry[]): void | Promise<void> {
    if (this.sink.emitBatch) return this.sink.emitBatch(entries);
    const pending: Promise<void>[] = [];
    for (const e of entries) {
      const r = this.sink.emit(e);
      if (isPromise(r)) pending.push(r);
    }
    return pending.length ? Promise.all(pending).then(() => undefined) : undefined;
  }

  /** Writes in the caller; a sink that answers with a promise settles later. */
  write(entry: Entry): void {
    if (this.onFail !== "panic" && !this.up && !this.breaker.admit(now())) return;
    let r: void | Promise<void>;
    let n = 0;
    for (;;) {
      try {
        using _ = inside(this);
        r = this.sink.emit(entry);
        break;
      } catch (e) {
        if (transient(e) && n++ < this.retries) continue;
        if (this.onFail === "panic") throw new SerlogError(`sink \`${this.name}\` failed: ${message(e)}`, { cause: e });
        this.failed(e, this.tell);
        return;
      }
    }
    if (!isPromise(r)) {
      this.passed();
      return;
    }
    r.then(
      () => this.passed(),
      (e: unknown) =>
        this.onFail === "panic" ? crash(`sink \`${this.name}\` failed: ${message(e)}`) : this.failed(e, this.tell),
    );
  }

  async #attempt(entries: readonly Entry[], pause: boolean): Promise<void> {
    for (let n = 0; ; n++) {
      try {
        await adopt(this, () => this.#batch(entries));
        return;
      } catch (e) {
        if (!transient(e) || n >= this.retries) throw e;
        if (pause) await sleep(2 ** Math.min(n, 5));
      }
    }
  }

  /** Writes a batch from the queue, waiting out failures until it goes through or is refused. */
  async drain(buf: LogRecord[], patient = true): Promise<void> {
    while (buf.length) {
      if (!this.up) {
        if (!patient) break;
        await sleep(this.breaker.wait(now()));
        this.breaker.admit(now());
      }
      const probing = !this.up;
      try {
        await this.#attempt(
          buf.map((r) => this.entry(r)),
          !probing,
        );
        this.passed();
        buf.length = 0;
      } catch (e) {
        if (this.onFail === "panic") await crash(`sink \`${this.name}\` failed: ${message(e)}`);
        this.failed(e, this.tell);
        if (!transient(e)) {
          this.dropped += buf.length;
          buf.length = 0;
        }
      }
    }
    this.dropped += buf.length;
    buf.length = 0;
  }

  /** Writes an alarm record straight to the sink; a failure rides in the incident being delivered. */
  async deliver(rec: LogRecord): Promise<void> {
    if (!this.up && !this.breaker.admit(now())) throw new SerlogError("down");
    try {
      await adopt(this, async () => {
        await this.#batch([this.entry(rec)]);
        await this.sink.flush?.();
      });
    } catch (e) {
      this.failed(e, "folded");
      throw e;
    }
    this.passed();
  }

  resting(): boolean {
    return !this.up && this.breaker.resting(now());
  }

  passed(): void {
    if (this.up) return;
    const phase = this.breaker.passed();
    if (phase) this.#notify(phase);
  }

  failed(e: unknown, tell: Tell): void {
    const remind = this.onFail instanceof Alarm ? resolve(this.onFail).remind : Number.POSITIVE_INFINITY;
    const phase = this.breaker.failed(now(), message(e), remind, tell);
    if (phase) this.#notify(phase);
  }

  #notify(phase: Phase): void {
    if (!(this.onFail instanceof Alarm)) return;
    const b = this.breaker;
    post(
      this.onFail,
      incident(this.name, this.kind, phase, b.error ?? "", {
        skipped: b.skippedNow,
        dropped: this.dropped,
        since: b.since,
      }),
    );
  }

  status(): Status {
    const b = this.breaker;
    return {
      name: this.name,
      kind: this.kind,
      health: b.health,
      skipped: b.skipped,
      dropped: this.dropped,
      suppressed: this.suppressed,
      since: b.since,
      error: b.error,
    };
  }
}

/** A slot's queue and the task draining it, both made when the first record arrives. */
export class Lane {
  readonly shape: Queue;
  readonly #core: Core;
  #buf: LogRecord[] = [];
  #task: Promise<void> | undefined;
  #due: { promise: Promise<void>; resolve: () => void } | undefined;

  constructor(shape: Queue, core: Core) {
    this.shape = shape;
    this.#core = core;
  }

  /** Queues `rec`; a full queue drops it, or with `full: block` keeps it while the sink is up. */
  push(rec: LogRecord): void {
    if (this.#buf.length >= this.shape.size && !(this.shape.full === "block" && this.#core.up)) {
      this.#core.dropped++;
      return;
    }
    this.#buf.push(rec);
    if (this.#buf.length >= this.shape.batch) this.#due?.resolve();
    this.#task ??= adopt(this.#core, () => this.#run());
  }

  async #run(): Promise<void> {
    while (this.#buf.length) {
      if (this.#buf.length < this.shape.batch) {
        this.#due = Promise.withResolvers<void>();
        const timer = setTimeout(this.#due.resolve, this.shape.wait * 1000);
        timer.unref();
        await this.#due.promise;
        clearTimeout(timer);
        this.#due = undefined;
      }
      const batch = this.#buf.splice(0, this.shape.batch);
      await this.#core.drain(batch);
    }
    this.#task = undefined;
  }

  /** Waits until what is queued has been written, unless the sink is down or the time is up. */
  async flush(within: number): Promise<void> {
    if (!this.#task || !this.#core.up) return;
    this.#due?.resolve();
    await Promise.race([this.#task, sleep(within)]);
  }
}

/** A registered slot. */
export class Live {
  readonly core: Core;
  readonly slot: Slot;
  readonly process: readonly Step[];
  readonly claim: string | undefined;
  readonly queue: Queue | undefined;
  readonly lane: Lane | undefined;

  constructor(slot: Slot, name: string, named: ReadonlyMap<string, Alarm>) {
    const shape = slot.mode === "sink" ? slot.sink.queued?.() : slot.mode === "direct" ? undefined : slot.mode;
    const retries = slot.retries ?? (shape ? 3 : 0);
    this.core = new Core(name, slot, policy(slot.onFail, named), retries);
    this.slot = slot;
    this.process = slot.process;
    this.claim = slot.sink.claim?.();
    this.queue = shape;
    this.lane = shape ? new Lane(shape, this.core) : undefined;
  }

  get name(): string {
    return this.core.name;
  }

  get alarmOnly(): boolean {
    return this.slot.alarmOnly;
  }

  /** Takes one record from the hub. */
  emit(rec: LogRecord, cache: Cache): void {
    if (this.slot.filter && !this.slot.filter.allows(rec.level, rec.target)) return;
    if (this.core.sink.enabled && !this.core.sink.enabled(rec.level, rec.target)) return;
    let r: LogRecord | null = rec;
    if (this.process.length) {
      using _ = inside();
      r = run(this.process, rec);
    }
    if (!r) return;
    if (this.lane) this.lane.push(r);
    else this.core.write(this.core.entry(r, r === rec ? cache : undefined));
  }

  /** Writes what the queue holds, then closes the sink. */
  async retire(): Promise<void> {
    await this.flush(5);
    try {
      await adopt(this.core, async () => this.core.sink.close?.());
    } catch {}
  }

  /** Waits for the queue, or flushes the sink. */
  async flush(within: number): Promise<void> {
    if (this.lane) return this.lane.flush(within);
    if (!this.core.up) return;
    try {
      await this.core.sink.flush?.();
    } catch (e) {
      this.core.failed(e, this.core.tell);
    }
  }
}
