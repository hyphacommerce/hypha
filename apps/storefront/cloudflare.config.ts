import { bindings, defineConfig, defineWorker } from "cf/config";

export default defineConfig({
  worker: defineWorker({
    name: "hypha-storefront",
    entrypoint: "vinext/server/fetch-handler",
    compatibilityDate: "2026-10-14",
    compatibilityFlags: ["nodejs_compat"],
    // As the admin's (see apps/admin/cloudflare.config.ts): no `runWorkerFirst`, so
    // static files are served directly and everything else reaches the Worker.
    assets: {
      htmlHandling: "none",
      notFoundHandling: "none",
    },
    env: {
      ASSETS: bindings.assets(),
      // Per deployment, so not in source: `.dev.vars` locally (see .env.example), Worker
      // secrets in production. Server-side only (design note 0023, SF2).
      HYPHA_API_URL: bindings.secret(),
      STOREFRONT_KEY: bindings.secret(),
    },
  }),
});
