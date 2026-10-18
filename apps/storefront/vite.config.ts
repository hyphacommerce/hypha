import { cloudflare } from "@cloudflare/vite-plugin";
import { kvDataAdapter } from "@vinext/cloudflare/cache/kv-data-adapter";
import { workersCacheCdnAdapter } from "@vinext/cloudflare/cache/workers-cache-cdn-adapter";
import vinext from "vinext";
import { defineConfig } from "vite";

// `HYPHA_TARGET=node` builds a standalone Node server instead (design note 0025): the
// Docker image for a VPS. `next.config.ts` asks for that output under the same flag.
const node = process.env.HYPHA_TARGET === "node";

// The admin's setup (design note 0011, P2): vinext runs the Next.js app on Vite, and
// the Cloudflare plugin runs it in workerd, as it will run in production (0023, SF1).
export default defineConfig({
  server: { port: 3002 },
  preview: { port: 3002 },
  plugins: [
    // On Workers, ISR pages in the Workers Cache and `unstable_cache` data in KV, shared
    // by every isolate (cloudflare.config.ts binds them; design note 0026, §2). On Node
    // one process holds them in memory.
    vinext(node ? {} : { cache: { cdn: workersCacheCdnAdapter(), data: kvDataAdapter() } }),
    !node &&
      cloudflare({
        // The admin's worker debugger takes the default, 9229, so both can run (`pnpm dev`).
        inspectorPort: 9230,
        viteEnvironment: {
          name: "rsc",
          childEnvironments: ["ssr"],
        },
      }),
  ],
});
