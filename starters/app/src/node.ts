// The Node host.
//
//   APP_URL=http://localhost:8787 pnpm dev
//
// Hypha must be allowed to call it: on a local API, `outbound.allow_private = true` in
// hypha.config.toml (or HYPHA_ALLOW_PRIVATE_CALLBACKS=true).

import { serve } from "@hono/node-server";

import { createApp } from "./app.ts";
import { MemoryInstallations } from "./installations.ts";

const appUrl = process.env.APP_URL ?? "http://localhost:8787";
const port = Number(process.env.PORT ?? new URL(appUrl).port ?? 8787);
const app = createApp({
  appUrl,
  installations: new MemoryInstallations(),
  log: (message, fields = {}) => console.log(JSON.stringify({ message, ...fields })),
});

serve({ fetch: app.fetch, port }, () => console.log(`listening on ${port} as ${appUrl}`));
