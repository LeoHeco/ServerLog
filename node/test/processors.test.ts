import assert from "node:assert/strict";
import { test } from "node:test";
import { Dedupe, Exclude, Fields, field, Level, Limit, Match, Redact, record, Sample, Sanitise } from "../src/index.ts";
import type { Extras, LogRecord } from "../src/record.ts";

const rec = (msg = "x", extras: Extras & { level?: number } = {}): LogRecord => {
  const { level = Level.INFO, ...rest } = extras;
  return record(level, "t", msg, rest);
};

test("redacts keys, paths and patterns", () => {
  const r = rec("call 13800138000 now", {
    fields: [
      ["pass", "hunter2"],
      ["req", { headers: { authorization: "Bearer x", accept: "*/*" } }],
      ["user", "ann"],
    ],
  });
  const out = new Redact({ keys: ["pass"], paths: ["req.headers.authorization"], patterns: ["\\d{11}"] }).process(r);
  assert.equal(field(out, "pass"), "***");
  assert.equal(field(out, "user"), "ann");
  assert.deepEqual(field(out, "req"), { headers: { authorization: "***", accept: "*/*" } });
  assert.equal(out.msg, "call *** now");
  assert.equal(field(r, "pass"), "hunter2");
});

test("leaves clean records alone", () => {
  const r = rec("fine", { fields: [["n", 1]] });
  assert.equal(new Redact({ keys: ["pass"] }).process(r), r);
  assert.equal(new Sanitise().process(r), r);
});

test("sanitises line breaks", () => {
  const out = new Sanitise().process(rec("x\r\ny\x07", { fields: [["who", "a\nb"]] }));
  assert.equal(out.msg, "x\\r\\ny\\u{0007}");
  assert.equal(field(out, "who"), "a\\nb");
});

test("adds, matches and excludes", () => {
  assert.equal(field(new Fields({ region: "cn" }).process(rec()), "region"), "cn");
  const r = rec("x", { level: Level.WARN, tags: ["audit"], fields: [["mirror", "debian"]] });
  assert.equal(new Match({ tags: ["audit"], fields: { mirror: "debian" }, level: "warn" }).process(r), r);
  assert.equal(new Match({ tags: ["other"] }).process(r), null);
  assert.equal(new Exclude({ tags: ["audit"] }).process(r), null);
  assert.equal(new Exclude({ level: "error" }).process(r), r);
});

test("samples, limits and dedupes", async () => {
  const s = new Sample({ first: 2, every: 3, per: "1m" });
  assert.deepEqual(
    Array.from({ length: 8 }, () => s.process(rec("same")) !== null),
    [true, true, false, false, true, false, false, true],
  );
  const lim = new Limit({ rate: 0, burst: 3 });
  assert.equal(Array.from({ length: 5 }, () => lim.process(rec())).filter(Boolean).length, 3);
  const d = new Dedupe({ within: "200ms" });
  assert.ok(d.process(rec("again")));
  assert.equal(d.process(rec("again")), null);
  assert.equal(d.process(rec("again")), null);
  await new Promise((r) => setTimeout(r, 250));
  const next = d.process(rec("again"));
  assert.ok(next);
  assert.equal(field(next, "repeated"), 2);
});
