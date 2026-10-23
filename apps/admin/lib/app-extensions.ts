// Server-only: what installed apps add to this admin. One discovery read per request
// (`GET /admin/app-extensions`, every admin target), shared by the page and its parts;
// the targets are picked from it here.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { unstable_rethrow } from "next/navigation";

import { perRequest, sessionClient } from "./hypha";

export type Extensions = AdminSchemas["AdminExtensionsResponse"];
export type BlockExtension = AdminSchemas["BlockExtension"];
export type ActionExtension = AdminSchemas["ActionExtension"];
export type PageExtension = AdminSchemas["PageExtension"];
export type ExtensionContext = AdminSchemas["ExtensionContextDto"];

/** The admin targets this admin shows (the catalog in `AdminTarget`). */
export type AdminTarget =
  | "apps.page"
  | "settings.app"
  | "order.details"
  | "order.payment"
  | "order.fulfillment"
  | "product.details"
  | "customer.details"
  | "payment";

const NONE: Extensions = { apps: [], pages: [], blocks: [], actions: [], settings: [] };

/**
 * Every installed app's admin contributions, once per request. Nothing (rather than an
 * error) when the staff member can't read apps or the API can't answer: app UI never
 * breaks the page it's on.
 */
export const loadExtensions = perRequest(async (): Promise<Extensions> => {
  const client = await sessionClient();
  try {
    const { data } = await client.GET("/admin/app-extensions");
    return data ?? NONE;
  } catch (error) {
    // A redirect (an expired session) still goes through.
    unstable_rethrow(error);
    return NONE;
  }
});

/** The contributions to one target. */
export function forTarget(extensions: Extensions, target: AdminTarget) {
  return {
    blocks: extensions.blocks.filter((b) => b.target === target),
    actions: extensions.actions.filter((a) => a.target === target),
    pages: extensions.pages.filter((p) => p.target === target),
  };
}

/** An app's name by key, for labels; the key when discovery didn't list it. */
export function appName(extensions: Extensions, key: string): string {
  return extensions.apps.find((a) => a.key === key)?.name ?? key;
}

/** A block's tree, or why it isn't there. */
export type LoadedBlock = {
  app: string;
  id: string;
  appName: string;
  tree: AdminSchemas["BlockResponse"]["tree"] | null;
};

/**
 * The trees of `blocks` for `context`, fetched by the admin's server at once
 * (`GET /admin/app-extensions/blocks/{app}/{id}`). A block whose app fails, times out
 * or answers something that isn't a v1 tree comes back with `tree: null`.
 */
export async function loadBlocks(
  extensions: Extensions,
  blocks: BlockExtension[],
  context: ExtensionContext,
): Promise<LoadedBlock[]> {
  if (blocks.length === 0) return [];
  const client = await sessionClient();
  const query = Object.fromEntries(
    Object.entries(context).filter(([, value]) => typeof value === "string"),
  );
  return Promise.all(
    blocks.map(async (block) => {
      let tree: LoadedBlock["tree"] = null;
      try {
        const { data } = await client.GET("/admin/app-extensions/blocks/{app}/{id}", {
          params: { path: { app: block.app, id: block.id }, query },
        });
        tree = data?.tree ?? null;
      } catch (error) {
        unstable_rethrow(error);
        // Unreachable API: shown like an app that didn't answer.
      }
      return { app: block.app, id: block.id, appName: appName(extensions, block.app), tree };
    }),
  );
}

/** The admin path an app's `navigate` goes to: its first page, at `path` within it. */
export function appPagePath(extensions: Extensions, app: string, path: string): string | null {
  const page = extensions.pages.find((p) => p.app === app && p.target === "apps.page");
  if (!page) return null;
  const query = path === "/" ? "" : `?path=${encodeURIComponent(path)}`;
  return `/apps/${encodeURIComponent(app)}/pages/${encodeURIComponent(page.id)}${query}`;
}
