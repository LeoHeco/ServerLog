import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { test } from "node:test";

const DIST = new URL("../dist/", import.meta.url);
const built = existsSync(new URL("index.mjs", DIST)) && existsSync(new URL("index.cjs", DIST));

const node = (args: string[]): string => {
  const p = spawnSync(process.execPath, args, {
    cwd: new URL("..", import.meta.url),
    encoding: "utf8",
    env: { ...process.env, NO_COLOR: "1" },
  });
  assert.equal(p.status, 0, p.stderr);
  return p.stdout;
};

test("the built package loads as ESM and CommonJS", { skip: !built && "run `npm run build` first" }, () => {
  const esm = node(["--input-type=module", "-e", 'import * as s from "serlog"; s.info("from esm");']);
  const cjs = node(["-e", 'const s = require("serlog"); s.info("from cjs");']);
  assert.match(esm, /INFO {2}main: from esm\n$/);
  assert.match(cjs, /INFO {2}main: from cjs\n$/);
});
