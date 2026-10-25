import { defineConfig } from "tsdown";

// The published build: one ESM file for the `hypha` binary (package.json's
// publishConfig.bin). In the workspace, `pnpm hypha` runs the TypeScript source.
export default defineConfig({
  entry: { main: "./src/main.ts" },
  format: "esm",
  platform: "node",
  dts: false,
  clean: true,
});
