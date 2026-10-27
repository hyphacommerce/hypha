// One installation per store: its tokens (refreshed by `HyphaClient`) and the app's
// signing secret, handed out once at the first code exchange. This starter keeps them in
// memory, which is enough to try the app: replace `Installations` with your database
// (SQLite, D1, Postgres) before anyone installs it for real, since a restart forgets them.

import {
  HyphaClient,
  type InstallTokens,
  type TokenSet,
  type TokenStore,
  createAuthorizationUrl,
  exchangeCode,
  parseAuthorizationResponse,
} from "@hyphacommerce/app";

import { PATHS, SCOPES } from "./manifest.ts";

export interface Installation {
  /** `store_…`, as webhooks and extension calls name it in `Hypha-Store`. */
  store: string;
  /** The Hypha API this store is on. */
  apiUrl: string;
  tokens: TokenSet;
  /** Verifies everything Hypha sends: webhooks, extension calls, session tokens. */
  secret: string;
}

/** Where installations live. Implement it over your database. */
export interface Installations {
  get(store: string): Promise<Installation | undefined>;
  save(installation: Installation): Promise<void>;
  delete(store: string): Promise<void>;
  /** A pending authorization, by its `state`, until the browser comes back. */
  savePending(state: string, pending: { apiUrl: string; codeVerifier: string }): Promise<void>;
  takePending(state: string): Promise<{ apiUrl: string; codeVerifier: string } | undefined>;
}

export class MemoryInstallations implements Installations {
  readonly #rows = new Map<string, Installation>();
  readonly #pending = new Map<string, { apiUrl: string; codeVerifier: string }>();

  async get(store: string) {
    return this.#rows.get(store);
  }
  async save(installation: Installation) {
    this.#rows.set(installation.store, installation);
  }
  async delete(store: string) {
    this.#rows.delete(store);
  }
  async savePending(state: string, pending: { apiUrl: string; codeVerifier: string }) {
    this.#pending.set(state, pending);
  }
  async takePending(state: string) {
    const pending = this.#pending.get(state);
    this.#pending.delete(state);
    return pending;
  }
}

export class InstallError extends Error {}

/** Where to send a staff member's browser: Hypha's consent page for this app. */
export async function startInstall(
  installations: Installations,
  appUrl: string,
  api: string | undefined,
): Promise<string> {
  let apiUrl: URL;
  try {
    apiUrl = new URL(api ?? "");
  } catch {
    throw new InstallError("`api` must be the Hypha API's base URL");
  }
  const request = await createAuthorizationUrl({
    apiUrl: apiUrl.origin,
    manifestUrl: `${appUrl}${PATHS.manifest}`,
    redirectUri: `${appUrl}${PATHS.callback}`,
    scopes: SCOPES,
  });
  await installations.savePending(request.state, {
    apiUrl: apiUrl.origin,
    codeVerifier: request.codeVerifier,
  });
  return request.url;
}

/** Finishes an installation from the redirect's URL; answers the store's ID. */
export async function completeInstall(
  installations: Installations,
  appUrl: string,
  callbackUrl: string,
): Promise<string> {
  const response = parseAuthorizationResponse(callbackUrl);
  if (!response.ok) throw new InstallError(`Hypha refused: ${response.error}`);
  const pending = await installations.takePending(response.state);
  if (!pending) throw new InstallError("this authorization is unknown or was already used");
  const tokens: InstallTokens = await exchangeCode({
    apiUrl: pending.apiUrl,
    manifestUrl: `${appUrl}${PATHS.manifest}`,
    redirectUri: `${appUrl}${PATHS.callback}`,
    code: response.code,
    codeVerifier: pending.codeVerifier,
  });
  // The secret comes only with the first install; a reinstall keeps the one we have.
  const earlier = await installations.get(tokens.store);
  const secret = tokens.appSecret ?? earlier?.secret;
  if (!secret)
    throw new InstallError("no app secret: uninstall the app in Hypha and install it again");
  await installations.save({ store: tokens.store, apiUrl: pending.apiUrl, tokens, secret });
  return tokens.store;
}

/** The admin API as the app, for one store: tokens refreshed and stored as they rotate. */
export function clientFor(
  installations: Installations,
  appUrl: string,
  installation: Installation,
): HyphaClient {
  const tokens: TokenStore = {
    get: async () => (await installations.get(installation.store))?.tokens,
    set: async (next) => {
      const current = await installations.get(installation.store);
      if (current) await installations.save({ ...current, tokens: next });
    },
  };
  return new HyphaClient({
    apiUrl: installation.apiUrl,
    manifestUrl: `${appUrl}${PATHS.manifest}`,
    tokens,
  });
}
