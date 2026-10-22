// Calling the admin API as an app: an access token that refreshes itself. Refresh tokens
// rotate and a reused one revokes the grant, so refreshes are single-flight per client,
// and run inside the `TokenStore`'s `withLock` when it has one: several processes sharing
// one installation give it a lock that spans them (a database row lock, a Durable
// Object), and the refresh inside reads the tokens again first, so the process that
// waited uses the pair the other one stored.

import type { AdminPaths } from "@hypha/sdk/admin";
import createClient from "openapi-fetch";

import { refresh, type TokenSet } from "./oauth.ts";

/** Where an app keeps an installation's tokens (its database, KV, …). */
export interface TokenStore {
  get(): Promise<TokenSet | undefined>;
  set(tokens: TokenSet): Promise<void>;
  /**
   * Runs `fn` holding the lock named `key` (`HyphaClient` passes `REFRESH_LOCK`), so no
   * two refreshes of one installation overlap, across processes when the lock spans
   * them. Without it, refreshes are only single-flight within one client.
   */
  withLock?<T>(key: string, fn: () => Promise<T>): Promise<T>;
}

/** The lock `HyphaClient` refreshes under (`TokenStore.withLock`). */
export const REFRESH_LOCK = "hypha:refresh";

/** A `TokenStore` in memory, with an in-process lock: for tests, scripts and one process. */
export function memoryTokenStore(initial?: TokenSet): TokenStore {
  let tokens = initial;
  const held = new Map<string, Promise<unknown>>();
  return {
    get: async () => tokens,
    set: async (next) => {
      tokens = next;
    },
    async withLock<T>(key: string, fn: () => Promise<T>): Promise<T> {
      const before = held.get(key) ?? Promise.resolve();
      const run = before.then(fn, fn);
      const settled = run.then(
        () => undefined,
        () => undefined,
      );
      held.set(key, settled);
      try {
        return await run;
      } finally {
        if (held.get(key) === settled) held.delete(key);
      }
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

  /** Both paths refresh here: an expiring token (`accessToken`) and a 401 (`fetch`). */
  #refresh(current: TokenSet): Promise<TokenSet> {
    this.#refreshing ??= (async () => {
      try {
        const store = this.#tokens;
        const run = () => this.#refreshUnlocked(current);
        return await (store.withLock ? store.withLock(REFRESH_LOCK, run) : run());
      } finally {
        this.#refreshing = undefined;
      }
    })();
    return this.#refreshing;
  }

  async #refreshUnlocked(current: TokenSet): Promise<TokenSet> {
    // Another flight (or process, while this one waited for the lock) may have stored
    // newer tokens since `current` was read.
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
  }
}
