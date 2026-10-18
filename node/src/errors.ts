/** What can go wrong: at start-up, while registering a sink, and inside a sink. */

/** Anything serlog throws. */
export class SerlogError extends Error {
  override name = "SerlogError";
}

/** A setting is missing or malformed. */
export class ConfigError extends SerlogError {
  override name = "ConfigError";
}

/** A config names a kind nobody registered. */
export class KindError extends ConfigError {
  override name = "KindError";
}

/** Two sinks share a name, or a name is reserved. */
export class NameTaken extends SerlogError {
  override name = "NameTaken";
}

/** A resource, such as a file, is already claimed by another sink. */
export class Claimed extends SerlogError {
  override name = "Claimed";
}

/** A sink failed its start-up check. */
export class NotReady extends SerlogError {
  override name = "NotReady";
  readonly sink: string;

  constructor(sink: string, cause: unknown) {
    super(`sink \`${sink}\` is not ready: ${Error.isError(cause) ? cause.message : String(cause)}`, { cause });
    this.sink = sink;
  }
}

/** The other end refused the records; sending them again will not help. */
export class Rejected extends SerlogError {
  override name = "Rejected";
}

/** `init` ran a second time. */
export class Twice extends SerlogError {
  override name = "Twice";

  constructor() {
    super("serlog is already initialised");
  }
}

/** Whether trying again could succeed. */
export const transient = (e: unknown): boolean => !(e instanceof Rejected || e instanceof ConfigError);

/** A message for any thrown value. */
export const message = (e: unknown): string => (Error.isError(e) ? e.message || e.name : String(e));
