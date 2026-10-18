import assert from "node:assert/strict";
import { afterEach, beforeEach, test } from "node:test";
import { fileURLToPath } from "node:url";
import * as serlog from "../src/index.ts";
import { clean, names } from "./help.ts";

let mem: serlog.Memory;
let before: string[];

beforeEach(async () => {
  before = names();
  serlog.setLevel("trace");
  mem = new serlog.Memory({ size: 100 });
  await serlog.add(mem, { filter: `calls=trace,${serlog.meta().app}=trace` });
});
afterEach(() => clean(before));

const last = (msg: string): serlog.LogRecord => {
  const r = mem.find((r) => r.msg === msg).at(-1);
  assert.ok(r, `no record "${msg}"`);
  return r;
};

test("messages, fields and the caller", function caller() {
  const sync = serlog.get("calls", { mirror: "debian" });
  sync.info({ tries: 3, ratio: 0.5, ok: true, tags: ["sync"] }, "synced %d files", 42);
  const r = last("synced 42 files");
  assert.deepEqual(r.fields, [
    ["mirror", "debian"],
    ["tries", 3],
    ["ratio", 0.5],
    ["ok", true],
  ]);
  assert.deepEqual(r.tags, ["sync"]);
  assert.equal(r.func, "caller");
  assert.equal(r.file, fileURLToPath(import.meta.url));
  assert.equal(r.module, "calls.test");
});

test("module calls use the app's name", () => {
  serlog.info("plain call");
  assert.equal(last("plain call").target, serlog.meta().app);
});

test("levels and aliases", () => {
  const log = serlog.get("calls");
  log.warning("w");
  log.fatal("c");
  log.log("debug", "d");
  log.log(35, "n");
  assert.deepEqual(
    ["w", "c", "d", "n"].map((m) => last(m).level),
    [serlog.Level.WARN, serlog.Level.FATAL, serlog.Level.DEBUG, 35],
  );
});

test("arguments are not worked out below the level", () => {
  serlog.setLevel("info");
  let hit = 0;
  serlog.get("calls").debug(
    "%s",
    serlog.lazy(() => hit++),
  );
  assert.equal(hit, 0);
  serlog.get("calls").info(
    "%s",
    serlog.lazy(() => "worked"),
  );
  last("worked");
  serlog.get("calls").info({ n: serlog.lazy(() => 7) }, "lazy field");
  assert.deepEqual(last("lazy field").fields, [["n", 7]]);
});

test("bind spans awaits and callbacks", async () => {
  await serlog.bind({ request: 7 }, async () => {
    serlog.get("calls").info({ extra: 1 }, "inside");
    await new Promise((r) => setTimeout(r, 1));
    serlog.get("calls").info("after an await");
  });
  serlog.get("calls").info("outside");
  assert.deepEqual(last("inside").fields, [
    ["request", 7],
    ["extra", 1],
  ]);
  assert.deepEqual(last("after an await").fields, [["request", 7]]);
  assert.deepEqual(last("outside").fields, []);
});

test("children add fields", () => {
  serlog.get("calls", { a: 1 }).child({ b: 2 }).info("child");
  assert.deepEqual(last("child").fields, [
    ["a", 1],
    ["b", 2],
  ]);
});

test("exceptions carry the error", () => {
  serlog.get("calls").exception(new TypeError("bad input"), "parse failed");
  assert.ok(serlog.field(last("parse failed"), "error") instanceof TypeError);
});

test("catching logs and throws or swallows", async () => {
  const boom = serlog.catching(
    () => {
      throw new RangeError("no");
    },
    { target: "calls" },
  );
  assert.throws(boom, RangeError);
  const quiet = serlog.catching(
    async () => {
      throw new SyntaxError("k");
    },
    { reraise: false, target: "calls", level: "warn" },
  );
  assert.equal(await quiet(), undefined);
  const msgs = mem.records.filter((r) => r.target === "calls" && r.msg.includes("raised"));
  assert.equal(msgs.length, 2);
  assert.ok(msgs.some((r) => r.msg.includes("RangeError: no")));
  assert.ok(msgs.some((r) => r.level === serlog.Level.WARN && r.msg.includes("SyntaxError")));
});

test("console and DEBUG come through", async () => {
  assert.equal(serlog.debugDirectives("app:*,-app:noisy web"), "app:*=debug,app:noisy=off,web=debug");
  const all = new serlog.Memory();
  const name = await serlog.add(all, { filter: "console=trace" });
  {
    using _ = serlog.capture();
    console.log("from %s", "console");
    console.error("bad");
  }
  await serlog.remove(name);
  assert.deepEqual(
    all.records.map((r) => [r.level, r.msg]),
    [
      [serlog.Level.INFO, "from console"],
      [serlog.Level.ERROR, "bad"],
    ],
  );
});
