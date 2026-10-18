import { createWorkersCacheConfig } from "@vinext/cloudflare/cache/config";
import { bindings, defineConfig, defineWorker } from "cf/config";

// What `hypha deploy --target cloudflare` passes in (design note 0026, §5): the Worker's
// name and account, its domain, the API Worker to bind, the cache's KV namespace. Without
// them (`pnpm storefront:dev`, `vite preview`), a local Worker as before.
const deploy = process.env;
const api = deploy.HYPHA_CF_API_WORKER;

// Pages under a country are static with ISR (design note 0024, §7): on Workers their
// renders live in the Workers Cache and their data in KV, shared by every isolate, so the
// webhook's revalidation reaches them all (vite.config.ts names the adapters).
const workersCache = await createWorkersCacheConfig();

export default defineConfig({
  ...(deploy.HYPHA_CF_ACCOUNT_ID ? { accountId: deploy.HYPHA_CF_ACCOUNT_ID } : {}),
  worker: defineWorker({
    name: deploy.HYPHA_CF_WORKER ?? "hypha-storefront",
    entrypoint: "vinext/server/fetch-handler",
    compatibilityDate: "2026-10-14",
    compatibilityFlags: ["nodejs_compat"],
    ...workersCache,
    ...(deploy.HYPHA_CF_DOMAIN ? { domains: [deploy.HYPHA_CF_DOMAIN], workersDev: false } : {}),
    observability: { enabled: true },
    // As the admin's (see apps/admin/cloudflare.config.ts): no `runWorkerFirst`, so
    // static files are served directly and everything else reaches the Worker.
    assets: {
      htmlHandling: "none",
      notFoundHandling: "none",
    },
    env: {
      ASSETS: bindings.assets(),
      ...workersCache.env,
      VINEXT_KV_CACHE: bindings.kv(
        deploy.HYPHA_CF_CACHE_KV ? { id: deploy.HYPHA_CF_CACHE_KV } : undefined,
      ),
      // Per deployment, so not in source: `.dev.vars` locally (see .env.example), Worker
      // secrets in production. Server-side only (design note 0023, SF2).
      HYPHA_API_URL: bindings.secret(),
      STOREFRONT_KEY: bindings.secret(),
      HYPHA_WEBHOOK_SECRET: bindings.secret(),
      ...(deploy.STOREFRONT_ROUTING
        ? { STOREFRONT_ROUTING: bindings.text(deploy.STOREFRONT_ROUTING) }
        : {}),
      // The API through a Service Binding (lib/api-fetch.workerd.ts), with the
      // deployment's caller key, so the API believes the shopper's address we pass on (not the
      // binding's props: Cloudflare drops them for a Worker with a placement, as the API's).
      ...(api
        ? { API: bindings.worker({ worker: api }), HYPHA_CALLER_KEY: bindings.secret() }
        : {}),
    },
    exports: { ...workersCache.exports },
  }),
});
