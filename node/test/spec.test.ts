import assert from "node:assert/strict";
import { afterEach, beforeEach, test } from "node:test";
import { type Desk, type Incident, incident, sound, Tiers } from "../src/alarm/index.ts";
import { Filter } from "../src/filter.ts";
import { Json, LOGFMT, Pattern, PRETTY, type Schema } from "../src/layout/index.ts";
import { parseLevel } from "../src/level.ts";
import { detect, setMeta } from "../src/meta.ts";
import { type LogRecord, record } from "../src/record.ts";
import { Breaker, type Tell } from "../src/slot/breaker.ts";
import { vectors } from "./help.ts";

const RENDER = vectors("render");

type Raw = {
  time: number;
  level: string;
  target: string;
  msg: string;
  file?: string;
  line?: number;
  fields: [string, unknown][];
};

function value(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(value);
  if (v && typeof v === "object") {
    const o = v as Record<string, unknown>;
    if (Array.isArray(o.$error)) {
      return (o.$error as string[]).reduceRight<Error | undefined>((cause, msg) => {
        const e = new Error(msg, cause && { cause });
        delete e.stack;
        return e;
      }, undefined);
    }
    return Object.fromEntries(Object.entries(o).map(([k, x]) => [k, value(x)]));
  }
  return v;
}

const make = (r: Raw): LogRecord =>
  record(parseLevel(r.level), r.target, r.msg, {
    fields: r.fields.map(([k, v]) => [k, value(v)] as const),
    ...(r.file && { file: r.file }),
    ...(r.line && { line: r.line }),
    time: new Date(r.time * 1000),
  });

beforeEach(() => setMeta(RENDER.meta));
afterEach(() => setMeta(detect()));

for (const c of RENDER.cases) {
  test(`renders ${c.record.msg.slice(0, 12)}`, () => {
    const rec = make(c.record);
    for (const p of c.patterns) assert.equal(new Pattern(p.pattern).render(rec), p.expect, p.pattern);
    for (const p of c.presets) {
      const got = new Pattern(p.name).render(rec);
      if ("expect" in p) assert.equal(got, p.expect, p.name);
      else assert.ok(got.endsWith(p.tail), `${p.name}: ${got}`);
    }
    if ("logfmt" in c) assert.equal(LOGFMT.render(rec), c.logfmt);
    if ("pretty" in c) assert.ok(PRETTY.render(rec).endsWith(c.pretty), PRETTY.render(rec));
    assert.equal(new Json().render(rec), c.json);
    for (const s of c.schemas) {
      const got = JSON.parse(new Json(s.schema as Schema).render(rec));
      assert.deepEqual(Object.keys(got), s.keys, s.schema);
      for (const [k, v] of Object.entries(s.has)) assert.deepEqual(got[k], v, `${s.schema}.${k}`);
    }
  });
}

for (const f of RENDER.filters) {
  test(`filter ${f.spec || "empty"}`, () => {
    const filter = new Filter(f.spec);
    for (const [key, want] of [
      ["allow", true],
      ["deny", false],
    ] as const) {
      for (const [lvl, target] of f[key])
        assert.equal(filter.allows(parseLevel(lvl), target), want, `${f.spec} ${lvl} ${target}`);
      for (const msg of f.messages?.[key] ?? []) assert.equal(filter.admits(msg), want, `${f.spec} ${msg}`);
    }
  });
}

for (const c of vectors("fail").cases) {
  test(`health ${c.name}`, () => {
    const b = new Breaker(c.probe, 0);
    for (const [i, step] of c.steps.entries()) {
      const tried = b.admit(step.at);
      assert.equal(tried, step.tried, `${c.name} #${i}`);
      const phase = tried
        ? step.result === "ok"
          ? b.passed()
          : b.failed(step.at, "boom", c.remind, c.tell as Tell)
        : undefined;
      assert.equal(b.health, step.health, `${c.name} #${i}`);
      assert.equal(phase ?? null, step.phase, `${c.name} #${i}`);
    }
    assert.equal(b.skipped, c.skipped);
  });
}

function byTier(c: { tiers: string[][]; logs: string[] }, tried: string[]): Set<string>[] {
  let left = [...tried];
  const out: Set<string>[] = [];
  for (const tier of c.tiers) {
    const members = new Set(tier.flatMap((n) => (n === "*" ? c.logs : [n])));
    const here = new Set(left.filter((n) => members.has(n)));
    left = left.filter((n) => !here.has(n));
    if (here.size) out.push(here);
  }
  return out;
}

for (const c of vectors("alarm").cases) {
  test(`alarm ${c.name}`, async () => {
    const tried: string[] = [];
    const desk: Desk = {
      logs: () => [...c.logs],
      resting: (n) => c.down.includes(n),
      deliver: async (n) => {
        tried.push(n);
        if (c.results[n] !== "ok") throw new Error("refused");
      },
    };
    const i: Incident = incident(c.failing, "custom", "opened", "boom");
    assert.equal(await sound(new Tiers({ tiers: c.tiers }), i, desk), c.outcome);
    assert.deepEqual(
      byTier(c, tried),
      c.tried.map((t: string[]) => new Set(t)),
    );
    assert.deepEqual(
      i.failures.map((f) => f.sink),
      c.failures,
    );
  });
}
