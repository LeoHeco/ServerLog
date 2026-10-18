/** Who is logging: application, environment and instance, fixed at start-up, and the host. */

import { hostname } from "node:os";
import { basename, extname } from "node:path";

/** Identity attached to records that leave the process. */
export interface Meta {
  readonly app: string;
  readonly env: string;
  readonly instance: string;
}

let host: string | undefined;

/** The host name, as far as the environment tells. */
export const hostName = (): string => (host ??= process.env.HOSTNAME || process.env.COMPUTERNAME || hostname());

/** The program name and the host name. */
export function detect(): Meta {
  const script = process.argv[1];
  return { app: script ? basename(script, extname(script)) : "", env: "", instance: hostName() };
}

let current: Meta = detect();

/** The identity given at start-up, or the defaults. */
export const meta = (): Meta => current;

/** Replaces the identity; `init` calls this. */
export function setMeta(m: Meta): void {
  current = m;
}
