/** Fields bound to the current async context. */

import { AsyncLocalStorage } from "node:async_hooks";
import type { Fields } from "./record.ts";

const store = new AsyncLocalStorage<Fields>();

/** Runs `fn` with `fields` added to every record made inside it, across awaits and callbacks alike. */
export function bind<T>(fields: object, fn: () => T): T {
  return store.run([...bound(), ...Object.entries(fields)], fn);
}

/** The fields bound here, in the order they were bound. */
export const bound = (): Fields => store.getStore() ?? [];
