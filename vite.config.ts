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
  "packages/app/src/generated/**",
  "apps/*/theme/*.css",
  "apps/*/theme/*.js",
  "apps/*/theme/*.d.ts",
  "openapi/**",
  "hypha.config.schema.json",
  "docs/apps/manifest.schema.json",
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
      // Sequential on purpose wherever it appears: paging by cursor, writes that must
      // happen in order and stop at the first error, uploads one at a time, deploy steps.
      "no-await-in-loop": "off",
      // Immutable updates of props, state and API data; the suggested `Object.assign`
      // would mutate shared objects.
      "oxc/no-map-spread": "off",
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
        files: [
          "packages/cli/**",
          "packages/emails/**",
          "scripts/**",
          "packages/sdk/scripts/**",
          "packages/app/scripts/**",
          "apps/*/scripts/**",
        ],
        plugins: ["node"],
        env: { node: true },
      },
      {
        // The app packages of design note 0029 run in browsers (the bridge, blocks and
        // elements) and on Workers (the SDK): Web APIs only.
        files: [
          "packages/app/src/**",
          "packages/app-bridge/src/**",
          "packages/app-bridge-host/src/**",
          "packages/app-blocks/src/**",
          "packages/storefront-elements/src/**",
        ],
        plugins: ["react", "jsx-a11y"],
        env: { browser: true },
        rules: {
          // The automatic JSX runtime needs no `React` in scope.
          "react/react-in-jsx-scope": "off",
          // Node's built-ins would tie them to Node; the tests may use them.
          "no-restricted-imports": [
            "error",
            {
              patterns: [
                {
                  group: ["node:*"],
                  message: "Web APIs only: these packages run in browsers and on Workers.",
                },
              ],
            },
          ],
        },
      },
      {
        files: [
          "packages/app*/src/**/*.test.ts",
          "packages/app-bridge/src/testing.ts",
          "packages/storefront-elements/src/**/*.test.ts",
        ],
        rules: {
          "no-restricted-imports": "off",
          // Tests stand in fakes for DOM types (windows, events, elements, registries) and
          // read JSON fixtures and responses whose shape is what they assert.
          "typescript/no-unsafe-type-assertion": "off",
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
