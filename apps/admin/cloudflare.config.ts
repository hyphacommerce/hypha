import { bindings, defineConfig, defineWorker } from "cf/config";

export default defineConfig({
  worker: defineWorker({
    name: "hypha-admin",
    entrypoint: "vinext/server/fetch-handler",
    compatibilityDate: "2026-10-14",
    compatibilityFlags: ["nodejs_compat"],
    // No `runWorkerFirst` (the vinext scaffold sets it to "/*"): with it, the Cloudflare
    // Vite plugin's dev server sent Vite's own requests (its client, CSS modules) to the
    // Worker too, so stylesheets came back as the app's pages. Without it, files that
    // exist as static assets are served directly and everything else reaches the
    // Worker, which is all the admin needs (it has no public files that shadow routes).
    assets: {
      htmlHandling: "none",
      notFoundHandling: "none",
    },
    env: {
      ASSETS: bindings.assets(),
      // Where the Hypha API runs. Per deployment, so not in source: `.dev.vars` locally
      // (see .env.example), a Worker secret in production. Server-side only.
      HYPHA_API_URL: bindings.secret(),
    },
  }),
});
