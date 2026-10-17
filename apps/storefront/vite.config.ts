import { cloudflare } from "@cloudflare/vite-plugin";
import vinext from "vinext";
import { defineConfig } from "vite";

// The admin's setup (design note 0011, P2): vinext runs the Next.js app on Vite, and
// the Cloudflare plugin runs it in workerd, as it will run in production (0023, SF1).
export default defineConfig({
  server: { port: 3002 },
  preview: { port: 3002 },
  plugins: [
    vinext(),
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
