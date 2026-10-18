import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, test } from "node:test";
import type { Entry, Sink, SlotOptions } from "../src/index.ts";
import * as serlog from "../src/index.ts";
import { clean, names } from "./help.ts";

class Flaky implements Sink {
  readonly kind = "flaky";
  readonly slot: SlotOptions;
  fail = false;
  seen = 0;

  constructor(slot: SlotOptions = {}) {
    this.slot = slot;
  }

  emit(): void {
    this.seen++;
    if (this.fail) throw new Error("boom");
  }
}

const status = (name: string): serlog.Status | undefined => serlog.status().find((s) => s.name === name);
const only = (target: string): string => `${target}=trace`;

let before: string[];
beforeEach(async () => {
  serlog.setLevel("trace");
  if (!names().includes("main")) await serlog.add(new serlog.Memory({ size: 1, name: "main", filter: "off" }));
  before = names();
});
afterEach(() => clean(before));

test("a failure is told once and its recovery after", async () => {
  const alarms = new serlog.Memory({ size: 10 });
  await serlog.add(alarms, { name: "alarms1", alarmOnly: true });
  const flaky = new Flaky({
    name: "flaky1",
    filter: only("t1"),
    probe: 0,
    onFail: new serlog.Tiers({ tiers: [["alarms1"]] }),
  });
  flaky.fail = true;
  await serlog.add(flaky);
  for (let i = 0; i < 3; i++) serlog.get("t1").info("write");
  await serlog.flush();
  assert.equal(flaky.seen, 3);
  assert.equal(status("flaky1")?.health, "down");
  const told = alarms.records;
  assert.equal(told.length, 1);
  assert.equal(told[0]?.level, serlog.Level.ERROR);
  assert.equal(told[0]?.msg, "sink `flaky1` failed: boom");
  assert.equal(serlog.field(told[0] as serlog.LogRecord, "phase"), "opened");
  flaky.fail = false;
  serlog.get("t1").info("write");
  await serlog.flush();
  assert.equal(status("flaky1")?.health, "up");
  assert.deepEqual(
    alarms.records.map((r) => serlog.field(r, "phase")),
    ["opened", "resolved"],
  );
});

test("a down sink is skipped until its probe", async () => {
  const flaky = new Flaky();
  flaky.fail = true;
  await serlog.add(flaky, { name: "flaky2", filter: only("t2"), probe: "1h", onFail: "ignore" });
  for (let i = 0; i < 3; i++) serlog.get("t2").info("write");
  assert.equal(flaky.seen, 1);
  const s = status("flaky2");
  assert.deepEqual([s?.health, s?.skipped, s?.error], ["down", 2, "boom"]);
});

test("the panic policy throws in the caller every time", async () => {
  const flaky = new Flaky();
  flaky.fail = true;
  await serlog.add(flaky, { name: "flaky3", filter: only("t3"), onFail: "panic" });
  for (let i = 0; i < 2; i++) assert.throws(() => serlog.get("t3").error("write"), /flaky3/);
  assert.equal(flaky.seen, 2);
});

test("names, claims and checks are enforced", async () => {
  const a = await serlog.add(new serlog.Memory({ size: 1, filter: "off" }));
  const b = await serlog.add(new serlog.Memory({ size: 1, filter: "off" }));
  assert.ok(a.startsWith("memory-") && b.startsWith("memory-") && a !== b);
  await assert.rejects(serlog.add(new serlog.Memory(), { name: "main" }), serlog.NameTaken);
  await assert.rejects(serlog.add(new serlog.Memory(), { name: "*" }), serlog.NameTaken);
  const dir = mkdtempSync(join(tmpdir(), "serlog-"));
  const path = join(dir, "one.log");
  await serlog.add(new serlog.File({ path, name: "one", filter: "off" }));
  await assert.rejects(serlog.add(new serlog.File({ path, name: "two" })), serlog.Claimed);
  await assert.rejects(serlog.add(new serlog.File({ path: dir, name: "three" })), serlog.NotReady);
  assert.ok(await serlog.remove("one"));
  assert.ok(!(await serlog.remove("one")));
});

test("a queued slot writes in the background", async () => {
  const mem = new serlog.Memory({ size: 100 });
  await serlog.add(mem, { name: "queued", filter: only("t5"), queue: 100, wait: "20ms" });
  for (let n = 0; n < 5; n++) serlog.get("t5").info({ n }, "queued");
  assert.equal(mem.length, 0);
  await serlog.flush();
  assert.equal(mem.length, 5);
});

test("slots lay out and process on their own", async () => {
  const js = new serlog.Memory({ size: 10 });
  const text = new serlog.Memory({ size: 10 });
  await serlog.add(js, { name: "json", filter: only("t6"), format: "json" });
  await serlog.add(text, {
    name: "text",
    filter: only("t6"),
    pattern: "{level}:{msg}",
    process: [{ match: { tags: ["keep"] } }],
  });
  const log = serlog.get("t6");
  log.info({ tags: ["keep"] }, "kept");
  log.info("dropped by text");
  assert.equal(js.length, 2);
  assert.ok(js.lines[0]?.startsWith('{"time":'));
  assert.deepEqual(text.lines, ["INFO:kept"]);
});

test("records made inside a sink are suppressed", async () => {
  let seen = 0;
  await serlog.add(
    () => {
      seen++;
      serlog.get("t7").info("from inside the sink");
    },
    { name: "loud", filter: only("t7") },
  );
  serlog.get("t7").info("outside");
  assert.equal(seen, 1);
  assert.equal(status("loud")?.suppressed, 1);
});

test("records made while an async sink works are suppressed", async () => {
  let seen = 0;
  const sink: Sink = {
    kind: "slow",
    emit: async () => {
      seen++;
      await new Promise((r) => setTimeout(r, 5));
      serlog.get("t9").info("from inside, later");
    },
  };
  await serlog.add(sink, { name: "slow", filter: only("t9"), queue: 10, wait: "1ms" });
  serlog.get("t9").info("outside");
  await serlog.flush();
  assert.equal(seen, 1);
  assert.equal(status("slow")?.suppressed, 1);
});

test("functions are sinks", async () => {
  const got: string[] = [];
  await serlog.add((e: Entry) => got.push(e.line), { name: "fn", filter: only("t8"), pattern: "{msg}" });
  serlog.get("t8").warn("hi");
  assert.deepEqual(got, ["hi"]);
});

test("removing a slot writes its queue and closes its sink", async () => {
  const mem = new serlog.Memory({ size: 10 });
  await serlog.add(mem, { name: "leaving", filter: only("t11"), queue: 10, wait: "1h" });
  for (let i = 0; i < 3; i++) serlog.get("t11").info("queued");
  const path = join(mkdtempSync(join(tmpdir(), "serlog-")), "gone.log");
  await serlog.add(new serlog.File({ path, name: "gone", filter: "off" }));
  assert.ok((await serlog.remove("leaving")) && (await serlog.remove("gone")));
  assert.equal(mem.length, 3);
  rmSync(path);
});
