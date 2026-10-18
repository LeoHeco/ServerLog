/** A slot's health: up, down, or probing after a failure; free of I/O so the shared vectors can drive it. */

/** Whether a sink is working. */
export type Health = "up" | "down" | "probing";

/** How a failure is told. */
export type Tell = "report" | "silent" | "folded";

/** Which report of an incident this is. */
export type Phase = "opened" | "reminder" | "resolved";

const LONGEST = 300;

/** The state machine behind one slot; times are seconds on a monotonic clock. */
export class Breaker {
  health: Health = "up";
  since: Date = new Date();
  skipped = 0;
  error: string | undefined;
  readonly #base: number;
  #gap: number;
  #next: number;
  #told: number | undefined;
  #openedAt = 0;

  constructor(probe: number, now: number) {
    this.#base = this.#gap = probe;
    this.#next = now;
  }

  /** Records skipped since the current failure began. */
  get skippedNow(): number {
    return this.skipped - this.#openedAt;
  }

  /** Whether a write may go ahead; turns a due `down` into `probing`, and counts a skip otherwise. */
  admit(now: number): boolean {
    if (this.health === "up") return true;
    if (this.health === "down" && now >= this.#next) {
      this.health = "probing";
      return true;
    }
    this.skipped++;
    return false;
  }

  /** Whether the sink is left out at `now`, without counting anything. */
  resting(now: number): boolean {
    return this.health === "probing" || (this.health === "down" && now < this.#next);
  }

  /** Seconds until the next probe. */
  wait(now: number): number {
    return Math.max(this.#next - now, 0);
  }

  /** A write succeeded; `resolved` when the failure had been told. */
  passed(): Phase | undefined {
    if (this.health === "up") return undefined;
    this.health = "up";
    this.#gap = this.#base;
    this.since = new Date();
    this.error = undefined;
    const told = this.#told;
    this.#told = undefined;
    return told === undefined ? undefined : "resolved";
  }

  /** A write failed; `opened` or `reminder` when that is to be told. */
  failed(now: number, error: string, remind: number, tell: Tell): Phase | undefined {
    this.error = error;
    if (this.health === "up") {
      this.health = "down";
      this.#gap = this.#base;
      this.#next = now + this.#gap;
      this.since = new Date();
      this.#openedAt = this.skipped;
      if (tell === "silent") return undefined;
      this.#told = now;
      return tell === "report" ? "opened" : undefined;
    }
    if (this.health === "probing") {
      this.health = "down";
      this.#gap = Math.min(this.#gap * 2, LONGEST);
      this.#next = now + this.#gap;
      if (tell === "report" && this.#told !== undefined && now - this.#told >= remind) {
        this.#told = now;
        return "reminder";
      }
    }
    return undefined;
  }
}
