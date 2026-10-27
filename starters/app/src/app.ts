// The app's routes, on Hono: the manifest, the install flow, webhooks and the extension
// point. `createApp` takes what differs per host (Node, Workers): where installations
// live and the app's public URL.

import {
  OAuthError,
  type WebhookEnvelope,
  extensionHandler,
  webhookHandler,
} from "@hyphacommerce/app";
import { Hono } from "hono";

import {
  InstallError,
  type Installations,
  clientFor,
  completeInstall,
  startInstall,
} from "./installations.ts";
import { PATHS, manifest } from "./manifest.ts";
import { rates } from "./rates.ts";

export interface AppOptions {
  /** Where the app is served, without a trailing slash: the manifest is `APP_URL/manifest.json`. */
  appUrl: string;
  installations: Installations;
  log?: (message: string, fields?: Record<string, unknown>) => void;
}

export function createApp({ appUrl, installations, log = () => {} }: AppOptions): Hono {
  const app = new Hono();
  /** The secret that verifies a request: the installation `Hypha-Store` names. */
  const secret = async ({ store }: { store: string | undefined }) =>
    store ? (await installations.get(store))?.secret : undefined;

  app.get("/health", (c) => c.json({ status: "ok" }));
  app.get("/", (c) => c.text(`${manifest(appUrl).name}: a Hypha app. Install it from your admin.`));
  app.get(PATHS.privacy, (c) =>
    c.text("This app stores each installation's tokens and the orders' IDs it's told about."),
  );
  app.get(PATHS.manifest, (c) => c.json(manifest(appUrl), 200, { "cache-control": "no-store" }));

  // The admin's "Install app" opens `<app_url>/install?api=<the Hypha API>`.
  app.get(PATHS.install, async (c) => {
    try {
      return c.redirect(await startInstall(installations, appUrl, c.req.query("api")), 302);
    } catch (error) {
      if (error instanceof InstallError) return c.text(error.message, 400);
      throw error;
    }
  });
  app.get(PATHS.callback, async (c) => {
    try {
      const store = await completeInstall(installations, appUrl, c.req.url);
      return c.text(`Installed for ${store}. You can close this tab.`);
    } catch (error) {
      if (error instanceof InstallError || error instanceof OAuthError)
        return c.text(`Installation failed: ${error.message}`, 400);
      throw error;
    }
  });

  // Webhooks: verified by the store's secret, answered 204; a thrown error is a 500 and
  // Hypha delivers again later.
  const webhooks = webhookHandler({
    secret,
    onEvent: async (event: WebhookEnvelope, { store }) => {
      if (!store) return;
      if (event.type === "app.uninstalled") {
        await installations.delete(store);
        return;
      }
      if (event.type === "order.placed") {
        // The API as the app: here, the order the event is about.
        const installation = await installations.get(store);
        if (!installation) return;
        const client = clientFor(installations, appUrl, installation);
        const order = await client.admin.GET("/admin/orders/{id}", {
          params: { path: { id: String(event.data.order) } },
        });
        log("order placed", { store, order: order.data?.number });
      }
    },
  });
  app.post(PATHS.webhooks, (c) => webhooks(c.req.raw));

  // Extension points: one handler per point the manifest names.
  const extensions = extensionHandler({ secret, handlers: [rates] });
  app.post(PATHS.extensions, (c) => extensions(c.req.raw));

  return app;
}
