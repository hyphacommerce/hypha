// React Doctor (`pnpm lint:react`): React-specific checks Oxlint doesn't have (effects that
// mirror props, derived state, refs read in render, sequential independent awaits in
// server code), on the two apps. Nothing leaves the machine: no score upload, share link,
// crash reports or Socket.dev supply-chain lookups (`pnpm lint:react` also passes
// `--no-telemetry`).

import { defineConfig } from "react-doctor/api";

export default defineConfig({
  projects: ["admin", "storefront"],
  noScore: true,
  share: false,
  supplyChain: { enabled: false },
  rules: {
    // Size and shape, not correctness: page components whose branches are the page's
    // steps, and tables that repeat a row layout.
    "react-doctor/no-high-complexity-react-function": "off",
    "react-doctor/no-giant-component": "off",
    "react-doctor/duplicate-jsx-subtree": "off",
    // `.includes` / `.find` over a product's options or a market's countries: lists of
    // a handful, where a Map would only add code.
    "react-doctor/js-set-map-lookups": "off",
    "react-doctor/js-index-maps": "off",
    // Edit forms seed their drafts from props on purpose.
    "react-doctor/no-derived-useState": "off",
    // Fires on `await params` then `await sessionClient()`, both request-local; the API
    // calls after them already go out together in `Promise.all`.
    "react-doctor/server-sequential-independent-await": "off",
  },
  ignore: {
    // Build output: vinext's Workers bundles and the Node build.
    files: [".cloudflare/**", "dist/**", "theme/*.js"],
    overrides: [
      {
        // The shop's server actions are the shopper's (a cart, a checkout): they run
        // with the publishable key, and the API checks what each may touch.
        files: ["app/[[]country]/actions.ts"],
        rules: ["react-doctor/server-auth-actions"],
      },
    ],
  },
});
