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
