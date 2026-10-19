export const LANGS = {
  rust: { label: "Rust", code: "rust" },
  python: { label: "Python", code: "python" },
  typescript: { label: "TypeScript", code: "ts" },
  java: { label: "Java", code: "java" },
  go: { label: "Go", code: "go" },
} as const;

export type Lang = keyof typeof LANGS;

export type Source = {
  file: string;
  code: string;
  full: string;
  regions: Record<string, string>;
  output: string;
};

export type Examples = Record<string, Record<Lang, Source>>;
