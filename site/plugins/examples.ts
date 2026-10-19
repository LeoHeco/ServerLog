import { readFile } from "node:fs/promises";
import path from "node:path";
import type { LoadContext, Plugin } from "@docusaurus/types";
import { type Examples, LANGS, type Lang, type Source } from "../src/langs.ts";

const NAMES = ["phone"];

const paths = (name: string): Record<Lang, string> => ({
  rust: `rust/examples/${name}.rs`,
  python: `python/examples/${name}.py`,
  typescript: `node/examples/${name}.ts`,
  java: `java/examples/${name[0]?.toUpperCase()}${name.slice(1)}.java`,
  go: `go/examples/${name}/${name}_test.go`,
});

const MARK = /^\s*(?:\/\/|#)\s*#?(end)?region\b\s*(\w*)/;

const dedent = (lines: string[]): string => {
  while (lines[0]?.trim() === "") lines.shift();
  while (lines.at(-1)?.trim() === "") lines.pop();
  const pad = Math.min(...lines.filter((l) => l.trim()).map((l) => l.length - l.trimStart().length));
  return lines.map((l) => l.slice(pad)).join("\n");
};

const squeeze = (lines: string[], gap: number): string =>
  dedent(lines)
    .replace(new RegExp(`\\n{${gap + 2},}`, "g"), "\n".repeat(gap + 1))
    .concat("\n");

function split(text: string, gap: number): Pick<Source, "full" | "regions"> {
  const open: string[] = [];
  const full: string[] = [];
  const parts: Record<string, string[]> = {};
  for (const line of text.split(/\r?\n/)) {
    const m = MARK.exec(line);
    if (m) {
      if (m[1]) open.pop();
      else open.push(m[2] ?? "");
      continue;
    }
    full.push(line);
    for (const r of open) (parts[r] ??= []).push(line);
  }
  return {
    full: squeeze(full, gap),
    regions: Object.fromEntries(Object.entries(parts).map(([k, v]) => [k, squeeze(v, gap).trimEnd()])),
  };
}

const goOutput = (text: string): string =>
  text
    .split(/\r?\n/)
    .reduce<{ on: boolean; lines: string[] }>(
      (acc, l) => {
        const t = l.trim();
        if (t === "// Output:") return { on: true, lines: [] };
        if (acc.on && t.startsWith("//")) acc.lines.push(t.replace(/^\/\/ ?/, ""));
        else if (acc.on) acc.on = false;
        return acc;
      },
      { on: false, lines: [] },
    )
    .lines.join("\n");

export default function examples(context: LoadContext): Plugin {
  const root = path.resolve(context.siteDir, "..");
  const files = NAMES.flatMap((n) => Object.values(paths(n)).map((p) => path.join(root, p)));
  return {
    name: "serlog-examples",
    getPathsToWatch: () => files.flatMap((f) => [f, f.replace(/\.\w+$/, ".out")]),
    async loadContent() {
      const all: Examples = {};
      for (const name of NAMES) {
        const entries = await Promise.all(
          Object.entries(paths(name)).map(async ([lang, file]) => {
            const text = await readFile(path.join(root, file), "utf8");
            const output =
              lang === "go"
                ? goOutput(text)
                : (await readFile(path.join(root, file.replace(/\.\w+$/, ".out")), "utf8")).trimEnd();
            const code = LANGS[lang as Lang].code;
            return [lang, { file, code, output, ...split(text, lang === "python" ? 2 : 1) }] as const;
          }),
        );
        all[name] = Object.fromEntries(entries) as Record<Lang, Source>;
      }
      return all;
    },
    async contentLoaded({ content, actions }) {
      actions.setGlobalData(content);
    },
  };
}
