import { cloudflare } from "@cloudflare/vite-plugin";
import vinext from "vinext";
import { defineConfig } from "vite";

// `HYPHA_TARGET=node` builds a standalone Node server instead: the Docker image for a
// VPS. `next.config.ts` asks for that output under the same flag.
const node = process.env.HYPHA_TARGET === "node";

// vinext runs the Next.js app on Vite; the Cloudflare plugin runs it in workerd, as it
// will run in production.
export default defineConfig({
  server: { port: 3001 },
  preview: { port: 3001 },
  // Development only. Every Astryx module is "use client", so a server component that
  // renders a Table (or any Astryx component) hands the browser a client reference to
  // the raw file, `dist/Table/Table.js`. vinext's dev dedup sends that through the
  // package's bare entry instead, here `@astryxdesign/core` itself, so the browser uses
  // the pre-bundled copy. Nothing in the admin imports that bare entry, so without this
  // the optimizer finds it only then, bundles it in a second pass and the browser holds
  // two copies of Astryx: the Table reads a translation context the `i18n` provider
  // never filled and renders `aria-label="Table"` against the server's "Tabela".
  // Listing the entries up front puts them in the first pass, sharing one copy of
  // Astryx's i18n. (`@vitejs/plugin-rsc`'s "client component dependency is
  // inconsistently optimized" names this pair of paths; its advice, excluding Astryx
  // from pre-bundling, would serve every Astryx file raw instead.) The client
  // environment's list only: vinext passes a top-level `optimizeDeps.include` to the
  // server environments too, where `@vitejs/plugin-rsc` keeps React libraries unbundled.
  environments: {
    client: {
      optimizeDeps: {
        include: ["@astryxdesign/core", "@astryxdesign/core/i18n", "@astryxdesign/core/Table"],
      },
    },
  },
  plugins: [
    vinext(),
    !node &&
      cloudflare({
        viteEnvironment: {
          name: "rsc",
          childEnvironments: ["ssr"],
        },
      }),
  ],
});
