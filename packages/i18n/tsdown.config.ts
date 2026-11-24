import { defineConfig } from "tsdown";

// The published build: ESM and declarations in dist/, one entry per export
// (package.json's publishConfig.exports), and the check's binary (publishConfig.bin),
// the one part that needs Node. In the workspace, exports and the binary point at the
// TypeScript source.
export default defineConfig([
  {
    entry: {
      index: "./src/index.ts",
      react: "./src/react.ts",
      check: "./src/check.ts",
    },
    format: "esm",
    platform: "neutral",
    dts: true,
    clean: true,
  },
  {
    entry: { bin: "./scripts/check.ts" },
    format: "esm",
    platform: "node",
    dts: false,
    clean: false,
  },
]);
