import type * as Preset from "@docusaurus/preset-classic";
import type { Config } from "@docusaurus/types";
import { themes } from "prism-react-renderer";
import examples from "./plugins/examples.ts";

const repo = "https://github.com/LeoHeco/ServerLog";

export default {
  title: "serlog",
  tagline: "Any number of sinks, each configured on its own, in Rust, Python, TypeScript, Java and Go.",
  favicon: "img/logo.svg",
  url: "https://leoheco.github.io",
  baseUrl: "/ServerLog/",
  organizationName: "LeoHeco",
  projectName: "ServerLog",
  trailingSlash: false,
  onBrokenLinks: "throw",
  onBrokenAnchors: "throw",
  future: { v4: true, faster: true },
  markdown: { mermaid: true, hooks: { onBrokenMarkdownLinks: "throw" } },
  themes: ["@docusaurus/theme-mermaid"],
  plugins: [examples],
  presets: [
    [
      "classic",
      {
        docs: { routeBasePath: "/", sidebarPath: "./sidebars.ts", editUrl: `${repo}/edit/main/site/` },
        blog: false,
        theme: { customCss: "./src/css/custom.css" },
      } satisfies Preset.Options,
    ],
  ],
  themeConfig: {
    colorMode: { respectPrefersColorScheme: true },
    navbar: {
      title: "serlog",
      logo: { alt: "", src: "img/logo.svg" },
      items: [
        { type: "docSidebar", sidebarId: "docs", position: "left", label: "Docs" },
        {
          type: "dropdown",
          label: "API",
          position: "left",
          items: [
            { label: "Rust", href: "https://docs.rs/serlog" },
            { label: "Python", href: "pathname:///ServerLog/api/python/serlog.html" },
            { label: "TypeScript", href: "pathname:///ServerLog/api/typescript/index.html" },
            { label: "Java", href: "https://javadoc.io/doc/io.github.leoheco/serlog" },
            { label: "Go", href: "https://pkg.go.dev/github.com/LeoHeco/ServerLog/go" },
          ],
        },
        { type: "custom-lang", position: "right" },
        { href: repo, position: "right", className: "navbar-github", "aria-label": "GitHub" },
      ],
    },
    docs: { sidebar: { hideable: true } },
    tableOfContents: { minHeadingLevel: 2, maxHeadingLevel: 3 },
    footer: {
      copyright: `MIT licence · <a href="${repo}">LeoHeco/ServerLog</a>`,
    },
    prism: {
      theme: themes.github,
      darkTheme: themes.vsDark,
      additionalLanguages: ["java", "toml", "bash", "powershell"],
    },
    mermaid: { theme: { light: "neutral", dark: "dark" } },
  } satisfies Preset.ThemeConfig,
} satisfies Config;
