import createClient from "openapi-fetch";

import type { components, paths } from "./generated/storefront";

export type StorefrontPaths = paths;
export type StorefrontSchemas = components["schemas"];

/**
 * A client for the storefront API with a publishable key (`hypha_pk_…`). Publishable
 * keys aren't secrets: this one can run in the browser.
 */
export function createStorefrontClient(baseUrl: string, publishableKey: string) {
  return createClient<paths>({
    baseUrl,
    headers: { "x-publishable-key": publishableKey },
  });
}

export type StorefrontClient = ReturnType<typeof createStorefrontClient>;
