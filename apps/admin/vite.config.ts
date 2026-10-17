import { cloudflare } from "@cloudflare/vite-plugin";
import vinext from "vinext";
import { defineConfig } from "vite";

// `HYPHA_TARGET=node` builds a standalone Node server instead (design note 0025): the
// Docker image for a VPS. `next.config.ts` asks for that output under the same flag.
const node = process.env.HYPHA_TARGET === "node";

// vinext runs the Next.js app on Vite; the Cloudflare plugin runs it in workerd, as it
// will run in production (design note 0011, P2).
export default defineConfig({
  server: { port: 3001 },
  preview: { port: 3001 },
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
