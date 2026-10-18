import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/index.ts"],
  format: ["esm", "cjs"],
  platform: "node",
  target: "node26",
  dts: true,
  clean: true,
});
