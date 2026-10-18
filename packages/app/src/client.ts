// Calling the admin API as an app (design note 0029, §2): an access token that refreshes
// itself. Refresh tokens rotate and a reused one revokes the grant, so refreshes are
// single-flight per client; several processes sharing one installation need their
// `TokenStore` to serialize refreshes too (a lock, a Durable Object).

import type { AdminPaths } from "@hypha/sdk/admin";
import createClient from "openapi-fetch";

import { refresh, type TokenSet } from "./oauth.ts";

/** Where an app keeps an installation's tokens (its database, KV, …). */
export interface TokenStore {
  get(): Promise<TokenSet | undefined>;
  set(tokens: TokenSet): Promise<void>;
}

/** A `TokenStore` in memory: for tests and scripts. */
export function memoryTokenStore(initial?: TokenSet): TokenStore {
  let tokens = initial;
  return {
    get: async () => tokens,
    set: async (next) => {
      tokens = next;
    },
  };
}

export interface HyphaClientOptions {
  /** The Hypha API's base URL. */
  apiUrl: string;
  /** The app's manifest URL (its OAuth client ID). */
  manifestUrl: string;
  tokens: TokenStore;
  fetch?: typeof globalThis.fetch;
  /** Refresh this long before expiry. Default 60 seconds. */
  refreshMarginMs?: number;
  /** Now in milliseconds; for tests. */
  now?: () => number;
}

/** No tokens stored: the app isn't installed (or was uninstalled). */
export class NotInstalledError extends Error {
  constructor() {
    super("no tokens for this installation: authorize the app first");
    this.name = "NotInstalledError";
  }
}

export class HyphaClient {
  readonly apiUrl: string;
  readonly manifestUrl: string;
  /** The admin API, typed from `@hypha/sdk`'s OpenAPI types, authenticated as the app. */
  readonly admin: ReturnType<typeof createClient<AdminPaths>>;
  readonly #tokens: TokenStore;
  readonly #fetch: typeof globalThis.fetch;
  readonly #margin: number;
  readonly #now: () => number;
  #refreshing: Promise<TokenSet> | undefined;

  constructor(options: HyphaClientOptions) {
    this.apiUrl = options.apiUrl;
    this.manifestUrl = options.manifestUrl;
    this.#tokens = options.tokens;
    this.#fetch = options.fetch ?? ((input, init) => globalThis.fetch(input, init));
    this.#margin = options.refreshMarginMs ?? 60_000;
    this.#now = options.now ?? Date.now;
    this.admin = createClient<AdminPaths>({
      baseUrl: this.apiUrl,
      fetch: (request: Request) => this.fetch(request),
    });
  }

  /** A valid access token, refreshed first when it's about to expire. */
  async accessToken(): Promise<string> {
    const tokens = await this.#tokens.get();
    if (!tokens) throw new NotInstalledError();
    if (tokens.expiresAt - this.#margin > this.#now()) return tokens.accessToken;
    return (await this.#refresh(tokens)).accessToken;
  }

  /**
   * `fetch` against the API with the app's token: a path (`/admin/orders`) or a Request.
   * A 401 refreshes once and retries, in case the token was revoked early.
   */
  async fetch(input: string | URL | Request, init?: RequestInit): Promise<Response> {
    const request =
      input instanceof Request
        ? new Request(input, init)
        : new Request(new URL(input, this.apiUrl), init);
    const retry = request.clone();
    const first = await this.#send(request, await this.accessToken());
    if (first.status !== 401) return first;
    const tokens = await this.#tokens.get();
    if (!tokens) return first;
    const renewed = await this.#refresh(tokens);
    return this.#send(retry, renewed.accessToken);
  }

  #send(request: Request, token: string): Promise<Response> {
    request.headers.set("authorization", `Bearer ${token}`);
    return this.#fetch(request);
  }

  #refresh(current: TokenSet): Promise<TokenSet> {
    this.#refreshing ??= (async () => {
      try {
        // Another flight may have stored newer tokens since `current` was read.
        const latest = (await this.#tokens.get()) ?? current;
        if (
          latest.refreshToken !== current.refreshToken &&
          latest.expiresAt - this.#margin > this.#now()
        )
          return latest;
        const next = await refresh({
          apiUrl: this.apiUrl,
          manifestUrl: this.manifestUrl,
          refreshToken: latest.refreshToken,
          fetch: this.#fetch,
          now: this.#now,
        });
        await this.#tokens.set(next);
        return next;
      } finally {
        this.#refreshing = undefined;
      }
    })();
    return this.#refreshing;
  }
}
