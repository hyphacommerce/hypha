import { bindings, defineConfig, defineWorker } from "cf/config";

// What `hypha deploy --target cloudflare` passes in (design note 0026, §5): the Worker's
// name and account, its domain, the API Worker to bind. Without them (`pnpm admin:dev`,
// `vite preview`), a local Worker as before.
const deploy = process.env;
const api = deploy.HYPHA_CF_API_WORKER;

export default defineConfig({
  ...(deploy.HYPHA_CF_ACCOUNT_ID ? { accountId: deploy.HYPHA_CF_ACCOUNT_ID } : {}),
  worker: defineWorker({
    name: deploy.HYPHA_CF_WORKER ?? "hypha-admin",
    entrypoint: "vinext/server/fetch-handler",
    compatibilityDate: "2026-10-14",
    compatibilityFlags: ["nodejs_compat"],
    ...(deploy.HYPHA_CF_DOMAIN ? { domains: [deploy.HYPHA_CF_DOMAIN], workersDev: false } : {}),
    observability: { enabled: true },
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
      // The API through a Service Binding (lib/api-fetch.workerd.ts); its props tell the
      // API the call is ours, so it believes the user's address the admin passes on.
      ...(api ? { API: bindings.worker({ worker: api, props: { caller: "admin" } }) } : {}),
    },
  }),
});
