/** Profiles that make serlog behave like another logger: its pattern, stream, default level and variables. */

/** Patterns that reproduce other loggers' default output; `log4j` is another name for `logback`. */
export const PATTERNS: Readonly<Record<string, string>> = {
  serlog: "{time} {level:<5} {target}: {msg}{fields}",
  tracing: "{time:rfc3339} {level:>5} {spans}{target}: {msg}{fields}",
  env_logger: "[{time:utc} {level:<5} {target}] {msg}",
  python: "{level}:{target}:{msg}",
  logback: "{time:%H:%M:%S%.3f} [{thread}] {level:<5} {target:~36} - {msg}",
  slog: "time={time:%Y-%m-%dT%H:%M:%S%.3f%:z} level={level} msg={msg:q}{fields:q}",
  pino: "[{time:%H:%M:%S%.3f}] {level} ({pid}): {msg}{fields}",
};

/** Where a console writes. */
export type Stream = "stdout" | "stderr" | "auto";

/** Another logger whose defaults serlog can take on. */
export type Compat = "serlog" | "tracing" | "env_logger" | "python" | "logback" | "slog" | "pino";

/** Every profile. */
export const COMPATS: readonly Compat[] = ["serlog", "tracing", "env_logger", "python", "logback", "slog", "pino"];

/** A profile by name; `log4j` means `logback`. */
export function parseCompat(name: string): Compat {
  const n = name === "log4j" ? "logback" : name;
  if (!(COMPATS as readonly string[]).includes(n)) throw new TypeError(`unknown compat profile \`${name}\``);
  return n as Compat;
}

/** Where a profile's console writes. */
export const streamOf = (c: Compat): Stream => (c === "env_logger" || c === "python" ? "stderr" : "stdout");

/** A profile's default level. */
export const levelOf = (c: Compat): number => ({ env_logger: 50, python: 40, logback: 20 })[c as string] ?? 30;

/** Environment variables a profile reads for the filter. */
export const envOf = (c: Compat): readonly string[] =>
  c === "serlog" ? ["SERLOG_FILTER"] : c === "tracing" || c === "env_logger" ? ["RUST_LOG"] : [];

let current: Compat = "serlog";

/** The profile in force. */
export const compat = (): Compat => current;

/** Puts a profile in force; `init` calls this. */
export function setCompat(c: Compat | string): void {
  current = parseCompat(c);
}
