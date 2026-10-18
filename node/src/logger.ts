/** Making records: loggers with a target and fields, the module-level calls, and catching errors. */

import { basename, dirname, extname } from "node:path";
import { fileURLToPath } from "node:url";
import { format, getCallSites } from "node:util";
import { emit, enabled } from "./hub.ts";
import { Level, type Levelish, parseLevel } from "./level.ts";
import { meta } from "./meta.ts";
import { type Fields, fieldsOf, type LogRecord, record } from "./record.ts";

/** A value worked out only when the record passes: `log.debug({ dump: serlog.lazy(expensive) }, "state")`. */
export class Lazy {
  readonly fn: () => unknown;

  constructor(fn: () => unknown) {
    this.fn = fn;
  }
}

/** Defers `fn` until a record is made. */
export const lazy = (fn: () => unknown): Lazy => new Lazy(fn);

const value = (v: unknown): unknown => (v instanceof Lazy ? v.fn() : v);
const here = dirname(fileURLToPath(import.meta.url));

function caller(): Pick<LogRecord, "file" | "line" | "func" | "module"> {
  for (const site of getCallSites(12)) {
    const file = site.scriptName.startsWith("file://") ? fileURLToPath(site.scriptName) : site.scriptName;
    if (!file || file.startsWith(here) || file.startsWith("node:")) continue;
    return {
      file,
      line: site.lineNumber,
      module: basename(file, extname(file)),
      ...(site.functionName && { func: site.functionName }),
    };
  }
  return {};
}

/** What every logging call takes: optional fields first, then a `util.format` message; `tags` lists tags. */
export type Args = [fields: object, msg?: string, ...args: unknown[]] | [msg: string, ...args: unknown[]];

/** A record from a logging call's arguments. */
export function make(level: number, target: string | undefined, bound: Fields, a: Args): LogRecord {
  const [first, ...rest] = a;
  const [own, msg, args]: [object, string, unknown[]] =
    typeof first === "string" ? [{}, first, rest] : [first ?? {}, String(rest[0] ?? ""), rest.slice(1)];
  const { tags, ...plain } = own as { tags?: string | string[] };
  return record(level, target ?? (meta().app || "main"), args.length ? format(msg, ...args.map(value)) : msg, {
    ...caller(),
    fields: [...bound, ...fieldsOf(plain).map(([k, v]) => [k, value(v)] as const)],
    tags: tags === undefined ? [] : [tags].flat(),
  });
}

/** Makes records for one target, with fields of its own; `get` hands these out. */
export class Logger {
  readonly #target: string | undefined;
  #fields: Fields;

  /** A logger for `target`, or the app's name when it is undefined. */
  constructor(target?: string, fields: object = {}) {
    this.#target = target;
    this.#fields = fieldsOf(fields);
  }

  /** A logger with more fields, and optionally another target. */
  child(fields: object, target: string | undefined = this.#target): Logger {
    const out = new Logger(target);
    out.#fields = [...this.#fields, ...fieldsOf(fields)];
    return out;
  }

  #at(level: number, a: Args): void {
    if (enabled(level)) emit(make(level, this.#target, this.#fields, a));
  }

  /** Logs at trace. */
  trace(...a: Args): void {
    this.#at(Level.TRACE, a);
  }

  /** Logs at debug. */
  debug(...a: Args): void {
    this.#at(Level.DEBUG, a);
  }

  /** Logs at info. */
  info(...a: Args): void {
    this.#at(Level.INFO, a);
  }

  /** Logs at warn. */
  warn(...a: Args): void {
    this.#at(Level.WARN, a);
  }

  /** Logs at warn, as Python's `logging` spells it. */
  warning(...a: Args): void {
    this.#at(Level.WARN, a);
  }

  /** Logs at error. */
  error(...a: Args): void {
    this.#at(Level.ERROR, a);
  }

  /** Logs at fatal. */
  fatal(...a: Args): void {
    this.#at(Level.FATAL, a);
  }

  /** Logs at `level`. */
  log(level: Levelish, ...a: Args): void {
    this.#at(parseLevel(level), a);
  }

  /** Logs `err` at error as the `error` field. */
  exception(err: unknown, msg?: string, ...args: unknown[]): void {
    this.#at(Level.ERROR, [{ error: err }, msg ?? (Error.isError(err) ? err.message : String(err)), ...args]);
  }
}

const root = new Logger();

/** A logger for `target`, carrying `fields` on every record. */
export const get = (target?: string, fields: object = {}): Logger => new Logger(target, fields);

/** Logs at trace through the default logger. */
export const trace: (...a: Args) => void = root.trace.bind(root);
/** Logs at debug through the default logger. */
export const debug: (...a: Args) => void = root.debug.bind(root);
/** Logs at info through the default logger. */
export const info: (...a: Args) => void = root.info.bind(root);
/** Logs at warn through the default logger. */
export const warn: (...a: Args) => void = root.warn.bind(root);
/** Logs at error through the default logger. */
export const error: (...a: Args) => void = root.error.bind(root);
/** Logs at fatal through the default logger. */
export const fatal: (...a: Args) => void = root.fatal.bind(root);
/** Logs at a level through the default logger. */
export const log: (level: Levelish, ...a: Args) => void = root.log.bind(root);
/** Logs an error through the default logger. */
export const exception: (err: unknown, msg?: string, ...args: unknown[]) => void = root.exception.bind(root);

/** Settings for [`catching`]. */
export interface CatchOptions {
  level?: Levelish;
  reraise?: boolean;
  target?: string;
}

/**
 * Logs any error `fn` throws or rejects with, then throws it again unless `reraise` is false. Works as a wrapper,
 * `catching(fn)`, and as a method decorator, `@catching`.
 */
export function catching<F extends (...a: never[]) => unknown>(
  fn: F,
  how?: CatchOptions | ClassMethodDecoratorContext,
): F {
  const opts: CatchOptions = how && "kind" in how ? {} : (how ?? {});
  const name = how && "kind" in how ? String(how.name) : fn.name || "anonymous";
  const level = parseLevel(opts.level ?? Level.ERROR);
  const report = (e: unknown): undefined => {
    if (enabled(level)) {
      const err = Error.isError(e) ? e : new Error(String(e));
      emit(
        record(level, opts.target ?? name, `${name} raised ${err.name}: ${err.message}`, { fields: [["error", err]] }),
      );
    }
    if (opts.reraise ?? true) throw e;
    return undefined;
  };
  return function (this: unknown, ...a: never[]) {
    try {
      const out = fn.apply(this, a);
      return out instanceof Promise ? out.catch(report) : out;
    } catch (e) {
      return report(e);
    }
  } as F;
}

/** Logs uncaught exceptions and unhandled rejections at fatal before Node's own handling. */
export function catchUncaught(): void {
  process.on("uncaughtExceptionMonitor", (e, origin) => {
    emit(
      record(Level.FATAL, "uncaught", `${origin}: ${Error.isError(e) ? e.message : String(e)}`, {
        fields: [["error", e]],
      }),
    );
  });
}
