/** Email for serious records; a batch goes out as one message, plain text and HTML. Needs the `nodemailer` package. */

import { compat } from "../compat.ts";
import { ConfigError, Rejected } from "../errors.ts";
import { strftime } from "../layout/clock.ts";
import { levelLabel, levelName } from "../level.ts";
import { meta } from "../meta.ts";
import { PAGE } from "../page.ts";
import { type Queue, queue } from "../queue.ts";
import type { Entry, Sink, SlotOptions } from "../sink.ts";
import { split } from "../sink.ts";
import { seconds, showTime } from "../units.ts";

/** Options of [`Mail`]; `pass` and `from` are the config keys, also spelt `password` and `sender`. */
export interface MailOptions extends SlotOptions {
  host: string;
  to?: string | readonly string[];
  port?: number;
  security?: "tls" | "starttls" | "none";
  user?: string;
  pass?: string;
  password?: string;
  from?: string;
  sender?: string;
  timeout?: number | string;
  connect?: boolean;
}

const html = (s: string): string =>
  s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);

/** Sends batches by SMTP; only `error` and above unless the slot sets a level or filter. */
export class Mail implements Sink {
  readonly kind = "mail";
  readonly slot: SlotOptions;
  readonly #o: Omit<MailOptions, keyof SlotOptions> & { to: readonly string[]; security: "tls" | "starttls" | "none" };
  readonly #timeout: number;
  readonly #min: number;

  constructor(options: MailOptions) {
    const [own, slot] = split(options);
    this.slot = slot;
    const { pass, password, from, sender, to, ...rest } = own;
    this.#o = {
      ...rest,
      ...((pass ?? password) && { pass: pass ?? password }),
      ...((from ?? sender) && { from: from ?? sender }),
      to: [to ?? []].flat(),
      security: own.security ?? "tls",
    };
    this.#timeout = seconds(own.timeout ?? 10);
    this.#min = slot.level !== undefined || slot.filter !== undefined ? 0 : 50;
  }

  #sender(): string {
    return this.#o.from ?? this.#o.user ?? "";
  }

  enabled(level: number): boolean {
    return level >= this.#min;
  }

  /** Subject, plain text and HTML for one batch. */
  render(entries: readonly Entry[]): { subject: string; text: string; html: string } {
    const m = meta();
    const top = Math.max(...entries.map((e) => e.record.level));
    const n = entries.length;
    const rows = entries
      .map((e) => {
        const r = e.record;
        return `<tr><td>${strftime(r.time, "%F %T")}</td><td class="${levelName(r.level)}">${levelLabel(r.level, compat())}</td><td>${html(r.target)}</td><td>${html(r.msg)}</td></tr>`;
      })
      .join("");
    const fill: Record<string, string> = {
      app: html(m.app),
      env: html(m.env),
      instance: html(m.instance),
      level: levelName(top),
      count: String(n),
      rows,
    };
    return {
      subject: `[${m.app}] ${n} ${levelLabel(top, compat())} record${n === 1 ? "" : "s"}`,
      text: entries.map((e) => `${e.line}\n`).join(""),
      html: PAGE.replace(/\{\{(\w+)\}\}/g, (all, k: string) => fill[k] ?? all),
    };
  }

  async #transport() {
    const { createTransport } = await import("nodemailer");
    const { security, port, user, pass, host } = this.#o;
    return createTransport({
      host,
      port: port ?? { tls: 465, starttls: 587, none: 25 }[security],
      secure: security === "tls",
      requireTLS: security === "starttls",
      ignoreTLS: security === "none",
      connectionTimeout: this.#timeout * 1000,
      ...(user && pass && { auth: { user, pass } }),
    });
  }

  emit(entry: Entry): Promise<void> {
    return this.emitBatch([entry]);
  }

  async emitBatch(entries: readonly Entry[]): Promise<void> {
    if (!entries.length) return;
    const t = await this.#transport();
    try {
      await t.sendMail({ from: this.#sender(), to: this.#o.to.join(", "), ...this.render(entries) });
    } catch (e) {
      const code = (e as { responseCode?: number }).responseCode;
      throw code !== undefined && code >= 500 ? new Rejected(`mail refused: ${code}`, { cause: e }) : e;
    }
  }

  async ready(): Promise<void> {
    if (!this.#o.to.length) throw new ConfigError("mail: no recipients");
    if (!this.#sender()) throw new ConfigError("mail: no sender");
    for (const a of [this.#sender(), ...this.#o.to])
      if (!/^[^\s@<>]+@[^\s@<>]+$/.test(a.replace(/^.*<(.*)>$/, "$1")))
        throw new ConfigError(`mail: bad address \`${a}\``);
    if (this.#o.user && !this.#o.pass) throw new ConfigError("mail: a user needs a password");
    if (this.#o.connect) await (await this.#transport()).verify();
  }

  queued(): Queue {
    return queue(1024, { wait: 60 });
  }

  options(): Record<string, unknown> {
    const o = this.#o;
    return {
      host: o.host,
      ...(o.port && { port: o.port }),
      ...(o.security !== "tls" && { security: o.security }),
      ...(o.user && { user: o.user, pass: "***" }),
      ...(o.from && { from: o.from }),
      to: [...o.to],
      ...(this.#timeout !== 10 && { timeout: showTime(this.#timeout) }),
      ...(o.connect && { connect: true }),
    };
  }
}
