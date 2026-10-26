// The Cloudflare Workers host (`wrangler dev`, `wrangler deploy`; see wrangler.toml.example).
// Installations in memory last one isolate: give the Worker a D1 database or KV namespace
// and implement `Installations` over it before deploying for real.

import { createApp } from "./app.ts";
import { type Installations, MemoryInstallations } from "./installations.ts";

interface Env {
  APP_URL: string;
}

let installations: Installations | undefined;

export default {
  fetch(request: Request, env: Env): Response | Promise<Response> {
    installations ??= new MemoryInstallations();
    const app = createApp({ appUrl: env.APP_URL, installations, log: console.log });
    return app.fetch(request);
  },
};
