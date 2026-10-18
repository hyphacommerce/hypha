// The TypeScript side's formatter and linter (Vite+: Oxfmt and Oxlint), for every app and
// package from here. `vp fmt`, `vp lint` and `vp check` read only this file: the apps'
// own `vite.config.ts` stay their vinext builds, and package-specific rules go in
// `overrides` below, never in a package's config.

import { defineConfig } from "vite-plus";

/** Written by tools, checked for staleness by their own commands. */
const generated = [
  "**/dist/**",
  "**/node_modules/**",
  "packages/sdk/src/generated/**",
  "apps/*/theme/*.css",
  "apps/*/theme/*.js",
  "apps/*/theme/*.d.ts",
  "openapi/**",
  "hypha.config.schema.json",
  "target/**",
  "spikes/**",
  "hypha-deploy*/**",
];

export default defineConfig({
  fmt: {
    printWidth: 100,
    // The design notes and READMEs wrap and align by hand; TOML is the Rust side's.
    ignorePatterns: [...generated, "**/*.md", "**/*.toml", "pnpm-lock.yaml"],
  },
  lint: {
    ignorePatterns: generated,
    plugins: ["typescript", "unicorn", "oxc", "import", "promise"],
    categories: {
      correctness: "error",
      suspicious: "warn",
      perf: "warn",
    },
    options: {
      typeAware: true,
    },
    rules: {
      // TypeScript checks what a function returns; a server action that ends in
      // `redirect()` (which never returns) isn't a missing value.
      "typescript/consistent-return": "off",
      // Theme declarations (`astryx theme build`) and `import "./globals.css"`.
      "unicorn/require-module-specifiers": "off",
      "import/no-unassigned-import": "off",
    },
    overrides: [
      {
        files: ["apps/**"],
        plugins: ["react", "jsx-a11y", "nextjs"],
        env: { browser: true },
        rules: {
          // The automatic JSX runtime needs no `React` in scope.
          "react/react-in-jsx-scope": "off",
          // Fires on openapi-fetch's `client.GET(…)`, which isn't a component.
          "react/capitalized-calls": "off",
          // Images are the store's media (S3/R2, sized by the API), not `next/image`'s.
          "nextjs/no-img-element": "off",
        },
      },
      {
        files: ["packages/cli/**", "scripts/**", "packages/sdk/scripts/**", "apps/*/scripts/**"],
        plugins: ["node"],
        env: { node: true },
        rules: {
          // Deploy steps and seeding run in order on purpose: each needs the last.
          "no-await-in-loop": "off",
        },
      },
      {
        // `node:test`'s `test()` returns a promise the runner awaits.
        files: ["**/*.test.ts"],
        rules: { "typescript/no-floating-promises": "off" },
      },
    ],
  },
});
