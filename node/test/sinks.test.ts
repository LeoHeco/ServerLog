import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, statSync, writeFileSync } from "node:fs";
import { createServer as http, type IncomingHttpHeaders } from "node:http";
import { createServer as tcp } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { gunzipSync } from "node:zlib";
import { ConfigError, Entry, File, Level, Loki, Mail, Pattern, Rejected, record } from "../src/index.ts";
import { PAGE } from "../src/page.ts";

const MSG = new Pattern("{msg}");
const dir = (): string => mkdtempSync(join(tmpdir(), "serlog-"));
const put = (f: File, msg: string, time = new Date(), level: number = Level.INFO): void =>
  f.emit(new Entry(record(level, "t", msg, { time }), MSG));
const ls = (d: string): string[] => readdirSync(d).toSorted();
const text = (p: string): string =>
  p.endsWith(".gz") ? gunzipSync(readFileSync(p)).toString() : readFileSync(p, "utf8");

test("rolls by size, keeps the newest and gzips", async () => {
  const d = dir();
  const f = new File({ path: join(d, "app.log"), maxSize: 150, keep: 2, gzip: true });
  f.ready();
  const start = Date.now();
  for (let i = 0; i < 12; i++) {
    put(f, `line ${String(i).padStart(2, "0")} ${"x".repeat(60)}`, new Date(start + i * 1000));
    await f.flush();
  }
  const rolled = ls(d).filter((n) => n.endsWith(".log.gz"));
  assert.equal(rolled.length, 2);
  assert.equal(ls(d).length, 3);
  assert.ok(text(join(d, "app.log")).includes("line 11"));
  const joined = rolled.map((n) => text(join(d, n))).join("");
  assert.ok(joined.includes("line 07") && joined.includes("line 09") && !joined.includes("line 05"));
});

test("rotates when the period turns", async () => {
  const d = dir();
  const f = new File({ path: join(d, "day.log"), rotate: "day" });
  f.ready();
  const now = new Date();
  put(f, "today", now);
  put(f, "still today", now);
  put(f, "tomorrow", new Date(now.getTime() + 25 * 3600_000));
  await f.flush();
  const old = ls(d).filter((n) => n !== "day.log");
  assert.equal(old.length, 1);
  assert.ok(text(join(d, old[0] as string)).includes("still today"));
  assert.equal(text(join(d, "day.log")), "tomorrow\n");
});

test("names rolled files from a template", async () => {
  const d = dir();
  const f = new File({ path: join(d, "svc.log"), maxSize: 10, rollName: "{stem}-old.{ext}" });
  f.ready();
  for (const m of ["first line", "second line", "third line"]) put(f, m);
  await f.flush();
  assert.deepEqual(ls(d), ["svc-old-1.log", "svc-old.log", "svc.log"]);
});

test("cleans up by total size and leaves others", async () => {
  const d = dir();
  writeFileSync(join(d, "big.yaml"), "x");
  const f = new File({ path: join(d, "big.log"), maxSize: 100, maxTotal: 250 });
  f.ready();
  for (let i = 0; i < 8; i++) {
    put(f, `${i} ${"y".repeat(97)}`);
    await f.flush();
  }
  const rolled = ls(d).filter((n) => n !== "big.log" && n !== "big.yaml");
  assert.ok(rolled.reduce((s, n) => s + statSync(join(d, n)).size, 0) <= 250);
  assert.ok(ls(d).includes("big.yaml"));
});

test("reopens after the file is moved away", async () => {
  const d = dir();
  const path = join(d, "moved.log");
  const f = new File({ path, reopen: true });
  f.ready();
  put(f, "before");
  await f.flush();
  renameSync(path, join(d, "moved.log.1"));
  await new Promise((r) => setTimeout(r, 1100));
  put(f, "after");
  await f.flush();
  assert.equal(text(path), "after\n");
});

test("truncates and makes directories", () => {
  const d = dir();
  const path = join(d, "t.log");
  writeFileSync(path, "old\n");
  const f = new File({ path, append: false });
  f.ready();
  put(f, "new");
  assert.equal(text(path), "new\n");
  new File({ path: join(d, "a", "b", "c.log") }).ready();
  assert.ok(statSync(join(d, "a", "b", "c.log")).isFile());
  assert.throws(() => new File({ path: join(d, "x", "y.log"), mkdir: false }).ready(), ConfigError);
});

test("buffers until an error", () => {
  const path = join(dir(), "buf.log");
  const f = new File({ path, buffer: "64KiB", flushEvery: "1h" });
  f.ready();
  put(f, "first");
  put(f, "second");
  assert.equal(text(path), "first\n");
  put(f, "bad", new Date(), Level.ERROR);
  assert.equal(text(path), "first\nsecond\nbad\n");
});

test("a file claims its canonical path", () => {
  const d = dir();
  mkdirSync(join(d, "x"));
  assert.equal(
    new File({ path: join(d, "x", "..", "same.log") }).claim(),
    new File({ path: join(d, "same.log") }).claim(),
  );
});

interface Seen {
  method: string;
  path: string;
  headers: IncomingHttpHeaders;
  body: string;
}

async function stand(
  answers: number[] = [],
): Promise<{ url: string; seen: Seen[]; [Symbol.asyncDispose](): Promise<void> }> {
  const seen: Seen[] = [];
  const server = http(async (req, res) => {
    const raw = Buffer.concat(await Array.fromAsync(req));
    seen.push({
      method: req.method ?? "",
      path: req.url ?? "",
      headers: req.headers,
      body: raw.length ? gunzipSync(raw).toString() : "",
    });
    res.writeHead(answers.shift() ?? 204).end();
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
  const { port } = server.address() as { port: number };
  return {
    url: `http://127.0.0.1:${port}/loki/api/v1/push`,
    seen,
    [Symbol.asyncDispose]: () => new Promise((r) => server.close(() => r())),
  };
}

test("loki pushes one stream per level", async () => {
  await using s = await stand();
  const loki = new Loki({ url: s.url, labels: { team: "mirrors" } });
  const lay = loki.defaultLayout();
  await loki.emitBatch(
    [
      record(Level.INFO, "sync", "started", {
        file: "a.ts",
        line: 3,
        fields: [
          ["mirror", "debian"],
          ["tries", 3],
        ],
      }),
      record(Level.ERROR, "sync", 'failed "rsync"'),
      record(Level.INFO, "sync", "retrying"),
    ].map((r) => new Entry(r, lay)),
  );
  const got = s.seen[0];
  assert.ok(got);
  assert.equal(got.path, "/loki/api/v1/push");
  assert.equal(got.headers["content-encoding"], "gzip");
  assert.equal(got.headers["content-type"], "application/json");
  const [info, err] = JSON.parse(got.body).streams;
  assert.deepEqual([info.stream.level, info.stream.team], ["info", "mirrors"]);
  assert.deepEqual(
    info.values.map((v: string[]) => v[1]),
    ["started", "retrying"],
  );
  assert.deepEqual(info.values[0][2], { target: "sync", file: "a.ts:3", mirror: "debian", tries: "3" });
  assert.equal(err.values[0][1], 'failed "rsync"');
});

test("loki tells retryable answers from refusals", async () => {
  await using s = await stand([503, 400]);
  const loki = new Loki({ url: s.url });
  const e = new Entry(record(Level.INFO, "t", "x"), loki.defaultLayout());
  await assert.rejects(loki.emit(e), (x: Error) => !(x instanceof Rejected) && x.message.includes("503"));
  await assert.rejects(loki.emit(e), Rejected);
});

test("loki checks its url and asks ready", async () => {
  await using s = await stand();
  await assert.rejects(new Loki({ url: "ftp://x" }).ready(), ConfigError);
  await new Loki({ url: s.url, connect: true }).ready();
  assert.deepEqual([s.seen.at(-1)?.method, s.seen.at(-1)?.path], ["GET", "/ready"]);
});

async function smtp(): Promise<{ port: number; data: string[]; [Symbol.asyncDispose](): Promise<void> }> {
  const data: string[] = [];
  const server = tcp((sock) => {
    let buf = "";
    let body: string[] | undefined;
    sock.write("220 test\r\n");
    sock.on("data", (chunk) => {
      buf += chunk.toString();
      for (let i = buf.indexOf("\r\n"); i >= 0; i = buf.indexOf("\r\n")) {
        const line = buf.slice(0, i + 2);
        buf = buf.slice(i + 2);
        if (body) {
          if (line === ".\r\n") {
            data.push(body.join(""));
            body = undefined;
            sock.write("250 queued\r\n");
          } else body.push(line);
          continue;
        }
        const cmd = line.toUpperCase();
        if (cmd.startsWith("EHLO")) sock.write("250-test\r\n250 OK\r\n");
        else if (cmd.startsWith("DATA")) {
          body = [];
          sock.write("354 go on\r\n");
        } else if (cmd.startsWith("QUIT")) sock.end("221 bye\r\n");
        else sock.write("250 OK\r\n");
      }
    });
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
  return {
    port: (server.address() as { port: number }).port,
    data,
    [Symbol.asyncDispose]: () => new Promise((r) => server.close(() => r())),
  };
}

test("mail sends one message per batch", async () => {
  await using s = await smtp();
  const m = new Mail({
    host: "127.0.0.1",
    port: s.port,
    security: "none",
    sender: "bot@example.org",
    to: "ops@example.org",
  });
  await m.ready();
  assert.ok(!m.enabled(Level.WARN));
  assert.ok(m.enabled(Level.ERROR));
  const lay = new Pattern("{level} {msg}");
  await m.emitBatch([
    new Entry(record(Level.ERROR, "sync", "disk <full>"), lay),
    new Entry(record(Level.FATAL, "sync", "mirror stalled"), lay),
  ]);
  const data = s.data[0] ?? "";
  assert.ok(data.includes("2 FATAL records"));
  assert.ok(data.includes("ERROR disk <full>"));
  assert.ok(data.includes("disk &lt;full&gt;"));
  assert.ok(data.includes("mirror stalled"));
});

test("mail refuses to start without addresses", async () => {
  for (const m of [
    new Mail({ host: "h", to: "ops@example.org" }),
    new Mail({ host: "h", sender: "bot@example.org" }),
    new Mail({ host: "h", user: "bot@example.org", to: "ops@example.org" }),
  ]) {
    await assert.rejects(m.ready(), ConfigError);
  }
});

test("the mail template matches the spec", () => {
  assert.equal(PAGE, readFileSync(new URL("../../spec/mail.html", import.meta.url), "utf8"));
});
