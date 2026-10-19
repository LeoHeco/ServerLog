import type { SidebarsConfig } from "@docusaurus/plugin-content-docs";

export default {
  docs: [
    "index",
    "start",
    "model",
    {
      type: "category",
      label: "Configure",
      collapsed: false,
      items: ["config", "sinks", "layouts", "processors", "compat"],
    },
    "failures",
    {
      type: "category",
      label: "Guides",
      collapsed: false,
      items: ["guides/phone", "guides/bridges"],
    },
  ],
} satisfies SidebarsConfig;
