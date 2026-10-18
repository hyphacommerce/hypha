import createClient from "openapi-fetch";

import type { components, paths } from "./generated/storefront";

export type StorefrontPaths = paths;
export type StorefrontSchemas = components["schemas"];

/** Extras for a server-side storefront: a `fetch` in place of the global one (a Worker's
 * Service Binding to the API), and headers such as `X-Forwarded-For`. */
export type StorefrontClientOptions = {
  fetch?: typeof globalThis.fetch;
  headers?: Record<string, string>;
};

/**
 * A client for the storefront API with a publishable key (`hypha_pk_…`). Publishable
 * keys aren't secrets: this one can run in the browser.
 */
export function createStorefrontClient(
  baseUrl: string,
  publishableKey: string,
  options: StorefrontClientOptions = {},
) {
  return createClient<paths>({
    baseUrl,
    ...(options.fetch ? { fetch: options.fetch } : {}),
    headers: { ...options.headers, "x-publishable-key": publishableKey },
  });
}

export type StorefrontClient = ReturnType<typeof createStorefrontClient>;
