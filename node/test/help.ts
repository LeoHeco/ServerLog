import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { remove, status } from "../src/hub.ts";

const SPEC = new URL("../../spec/", import.meta.url);

/** One of the shared vector files. */
// biome-ignore lint/suspicious/noExplicitAny: vectors are free-form JSON
export const vectors = (name: string): any => JSON.parse(readFileSync(new URL(`cases/${name}.json`, SPEC), "utf8"));

/** The package entry, as a script run elsewhere imports it. */
export const ENTRY: string = pathToFileURL(join(import.meta.dirname, "../src/index.ts")).href;

/** What a script printed and how it ended. */
export interface Ran {
  out: string;
  err: string;
  code: number;
}

/** Runs `code`, with `serlog` imported, in a new Node inside a fresh directory. */
export function run(
  code: string,
  env: Record<string, string> = {},
  cwd: string = mkdtempSync(join(tmpdir(), "serlog-")),
): Ran {
  const p = spawnSync(
    process.execPath,
    ["--input-type=module", "-e", `import * as serlog from ${JSON.stringify(ENTRY)};\n${code}`],
    {
      cwd,
      env: {
        ...Object.fromEntries(Object.entries(process.env).filter(([k]) => k !== "FORCE_COLOR")),
        NO_COLOR: "1",
        ...env,
      },
      encoding: "utf8",
      timeout: 60_000,
    },
  );
  return { out: p.stdout, err: p.stderr, code: p.status ?? -1 };
}

/** Removes the slots added since `before`. */
export async function clean(before: readonly string[]): Promise<void> {
  for (const s of status()) if (!before.includes(s.name)) await remove(s.name);
}

/** The names of the slots now registered. */
export const names = (): string[] => status().map((s) => s.name);
