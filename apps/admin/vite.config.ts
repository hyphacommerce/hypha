import { cloudflare } from "@cloudflare/vite-plugin";
import vinext from "vinext";
import { defineConfig } from "vite";

// vinext runs the Next.js app on Vite; the Cloudflare plugin runs it in workerd, as it
// will run in production (design note 0011, P2).
export default defineConfig({
  server: { port: 3001 },
  plugins: [
    vinext(),
    cloudflare({
      viteEnvironment: {
        name: "rsc",
        childEnvironments: ["ssr"],
      },
    }),
  ],
});
