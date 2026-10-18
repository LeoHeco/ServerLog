import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { expand, read } from "../src/config/index.ts";
import { ConfigError, KindError, SerlogError } from "../src/index.ts";
import { plan } from "../src/start.ts";
import { run } from "./help.ts";

const CONFIG = `
app: demo
level: trace
watch: 1s
levels:
  notice: { value: 35, colour: blue }
process:
  - fields: { region: cn }
redact: [pass]
alarms:
  ops:
    tiers: [[oncall], [stderr]]
    remind: 30m
sinks:
  - { kind: captured, label: all, name: all, format: json }
  - { kind: captured, label: notices, level: notice, pattern: "{level}:{msg}" }
  - { kind: captured, label: oncall, name: oncall, alarm_only: true }
  - { kind: file, name: app, path: "\${SERLOG_TEST_DIR}/app.log", max_size: 1MiB, keep: 3, on_fail: ops, queue: 100, wait: 50ms }
`;

const SCRIPT = `
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { setTimeout as sleep } from "node:timers/promises";

const captured = {};
class Captured extends serlog.Memory {
  kind = "captured";
  constructor({ label, ...rest }) {
    super({ size: 100, ...rest });
    this.label = label;
    captured[label] = this;
  }
  options() {
    return { label: this.label };
  }
}
serlog.kind("captured", (o) => new Captured(o));

writeFileSync("serlog.yaml", process.env.CONFIG);
await serlog.load("serlog.yaml");
serlog.info({ pass: "secret" }, "hello");
serlog.log(35, "notable");
await serlog.flush();
const out = { all: captured.all.lines, notices: captured.notices.lines, names: serlog.status().map((s) => s.name) };
out.file = readFileSync(join(process.env.SERLOG_TEST_DIR, "app.log"), "utf8");
out.yaml = await serlog.dump("yaml");
const trees = await Promise.all(["yaml", "json", "toml"].map(async (f) => serlog.config.read(await serlog.dump(f), f)));
out.same = isDeepStrictEqual(trees[0], trees[1]) && isDeepStrictEqual(trees[1], trees[2]);
await sleep(1100);
writeFileSync("serlog.yaml", "level: info\\nsinks:\\n  - { kind: captured, label: after, name: after }\\n");
await sleep(2500);
serlog.info("reloaded");
await serlog.flush();
out.after = captured.after.lines;
out.namesAfter = serlog.status().map((s) => s.name);
console.log(JSON.stringify(out));
`;

test("loads, dumps and reloads", () => {
  const dir = mkdtempSync(join(tmpdir(), "serlog-"));
  const r = run(SCRIPT, { CONFIG, SERLOG_TEST_DIR: dir });
  assert.equal(r.code, 0, r.err);
  const out = JSON.parse(r.out.trim().split("\n").at(-1) ?? "");
  assert.ok(
    out.all.some(
      (l: string) => l.includes('"msg":"hello"') && l.includes('"region":"cn"') && l.includes('"pass":"***"'),
    ),
    out.all,
  );
  assert.deepEqual(out.notices, ["NOTICE:notable"]);
  assert.ok(out.file.includes("hello"));
  assert.deepEqual(out.names, ["all", "captured-1", "oncall", "app"]);
  for (const want of [
    "notice",
    "region",
    "redact",
    "ops",
    "remind: 30m",
    "max_size: 1MiB",
    "on_fail: ops",
    "queue: 100",
  ]) {
    assert.ok(out.yaml.includes(want), want);
  }
  assert.ok(out.same);
  assert.ok(out.after.some((l: string) => l.endsWith("reloaded")));
  assert.deepEqual(out.namesAfter, ["after"]);
});

test("reads the same tree from every format", async () => {
  const y = await read("filter: info\nsinks:\n  - kind: console\n    stream: stderr\n", "yaml");
  const j = await read('{"filter":"info","sinks":[{"kind":"console","stream":"stderr"}]}', "json");
  const t = await read('filter = "info"\n[[sinks]]\nkind = "console"\nstream = "stderr"\n', "toml");
  assert.deepEqual(y, j);
  assert.deepEqual(j, t);
});

const MISTAKES: [Record<string, unknown>, RegExp][] = [
  [{ sinks: [{ kind: "console", colr: "never" }] }, /unknown key `colr`/],
  [{ bogus: 1 }, /unknown key `bogus`/],
  [{ sinks: [{ kind: "console", level: "info", filter: "info" }] }, /not both/],
  [{ sinks: [{ kind: "console", on_fail: "pager" }] }, /not in `alarms`/],
  [{ sinks: [{ kind: "console", format: "xml" }] }, /format/],
  [{ alarms: { panic: { tiers: [["stderr"]] } } }, /cannot name/],
  [{ process: [{ shout: {} }] }, /shout/],
  [{ alarms: { ops: { tiers: [["nobody"]] } }, sinks: [{ kind: "memory", on_fail: "ops" }] }, /nobody/],
];

for (const [tree, says] of MISTAKES) {
  test(`reports ${says.source}`, async () => {
    await assert.rejects(plan(tree, []), (e: Error) => e instanceof SerlogError && says.test(e.message));
  });
}

test("unknown kinds and variables", async () => {
  await assert.rejects(plan({ sinks: [{ kind: "kafka" }] }, []), KindError);
  assert.throws(
    // biome-ignore lint/suspicious/noTemplateCurlyInString: a config variable
    () => expand({ x: "${SERLOG_SURELY_UNSET}" }),
    (e: Error) => e instanceof ConfigError && e.message.includes("SERLOG_SURELY_UNSET"),
  );
});

test("zero config follows the environment", () => {
  const r = run('await serlog.init();\nserlog.debug("quiet");\nserlog.warn({ n: 1 }, "loud");\n', {
    SERLOG_FILTER: "warn",
    SERLOG_FORMAT: "json",
  });
  assert.equal(r.code, 0, r.err);
  const rec = JSON.parse(r.out.trim());
  assert.deepEqual([rec.level, rec.msg, rec.n], ["warn", "loud", 1]);
});

test("DEBUG adds namespaces at debug", () => {
  const r = run(
    'await serlog.init();\nserlog.get("app:db").debug("on");\nserlog.get("app:noisy").debug("off");\nserlog.get("web").debug("no");\n',
    {
      DEBUG: "app:*,-app:noisy",
    },
  );
  assert.equal(r.code, 0, r.err);
  assert.deepEqual(
    r.out
      .trim()
      .split("\n")
      .map((l) => l.slice(24)),
    ["DEBUG app:db: on"],
  );
});

test("profiles change pattern, stream and level", () => {
  const r = run(
    'await serlog.init({ compat: "python" });\nserlog.info("hidden");\nserlog.warn("shown");\nserlog.fatal("worse");\n',
  );
  assert.equal(r.code, 0, r.err);
  assert.equal(r.out, "");
  assert.deepEqual(r.err.trim().split(/\r?\n/), ["WARNING:main:shown", "CRITICAL:main:worse"]);
});

test("init runs once", () => {
  const r = run("await serlog.init();\nawait serlog.init();\n");
  assert.notEqual(r.code, 0);
  assert.ok(r.err.includes("already initialised"), r.err);
});

test("an empty config file starts the console", () => {
  const dir = mkdtempSync(join(tmpdir(), "serlog-"));
  const r = run(
    'import { writeFileSync } from "node:fs";\nwriteFileSync("serlog.yaml", "");\nawait serlog.init();\nserlog.info("up");\n',
    {},
    dir,
  );
  assert.equal(r.code, 0, r.err);
  assert.ok(r.out.endsWith("main: up\n"), r.out);
});

test("a background panic exits with 70", () => {
  const r = run(`
const bad = { kind: "bad", slot: { queue: 10, wait: 0.01, onFail: "panic", retries: 0 }, emit() { throw new Error("gone"); } };
await serlog.init({ sinks: [bad] });
serlog.info("x");
await new Promise((r) => setTimeout(r, 2000));
`);
  assert.equal(r.code, 70);
  assert.ok(r.err.includes("gone"), r.err);
});
