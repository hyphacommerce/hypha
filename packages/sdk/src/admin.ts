import createClient from "openapi-fetch";

import type { components, paths } from "./generated/admin";

export type AdminPaths = paths;
export type AdminSchemas = components["schemas"];

/** Every error the API returns: switch on `code` (stable), show `error` (for humans). */
export type ApiError = components["schemas"]["Error"];

/**
 * A client for the admin API. `token` is a staff session (`hypha_sess_…`) or a secret
 * key (`hypha_sk_…`); leave it out for the routes that need none (login, password
 * reset, invite acceptance).
 *
 * Only for server code: a staff session must never reach the browser (design note 0011,
 * P4). `headers` is for extras such as `X-Forwarded-For`.
 */
export function createAdminClient(
  baseUrl: string,
  token?: string,
  headers: Record<string, string> = {},
) {
  return createClient<paths>({
    baseUrl,
    headers: {
      ...headers,
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
  });
}

export type AdminClient = ReturnType<typeof createAdminClient>;
