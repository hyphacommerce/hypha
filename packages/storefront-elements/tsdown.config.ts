import { defineConfig } from "tsdown";

// The published build: ESM and declarations in dist/, one entry per export
// (package.json's publishConfig.exports). In the workspace, exports point at src/.
export default defineConfig({
  entry: {
    index: "./src/index.ts",
    react: "./src/react.ts",
  },
  format: "esm",
  platform: "neutral",
  dts: true,
  clean: true,
});
