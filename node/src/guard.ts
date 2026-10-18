/** Marks code running inside a sink or a processor, so records it makes are dropped instead of looping back. */

import { AsyncLocalStorage } from "node:async_hooks";

/** Something that counts suppressed records. */
export interface Counts {
  suppressed: number;
}

const marks = new AsyncLocalStorage<Counts | null>();
let depth = 0;
let owner: Counts | null = null;

/** Marks the synchronous rest of the scope as serlog's own code: `using _ = inside(core)`. */
export function inside(counts: Counts | null = null): Disposable {
  const was = owner;
  depth++;
  owner = counts;
  return {
    [Symbol.dispose]() {
      depth--;
      owner = was;
    },
  };
}

/** Runs `fn`, async work included, as serlog's own code. */
export const adopt = <T>(counts: Counts | null, fn: () => T): T => marks.run(counts, fn);

/** Counts and reports a record made inside serlog's own code, which must then be dropped. */
export function suppress(): boolean {
  if (depth > 0) {
    if (owner) owner.suppressed++;
    return true;
  }
  const mark = marks.getStore();
  if (mark === undefined) return false;
  if (mark) mark.suppressed++;
  return true;
}
