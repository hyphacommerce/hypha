import assert from "node:assert/strict";
import { test } from "node:test";

import {
  HyphaClient,
  NotInstalledError,
  REFRESH_LOCK,
  memoryTokenStore,
  type TokenStore,
} from "./client.ts";

const API = "https://api.shop.example";
const MANIFEST = "https://app.example/hypha-app.json";

/** An API that knows one valid access token at a time and rotates refresh tokens. */
function fakeApi() {
  let generation = 1;
  let refreshes = 0;
  const usedRefresh = new Set<string>();
  const calls: { url: string; authorization: string | null; body: string }[] = [];
  const fetch: typeof globalThis.fetch = async (input, init) => {
    const request = new Request(input, init);
    const url = new URL(request.url);
    if (url.pathname === "/oauth/token") {
      const form = new URLSearchParams(await request.text());
      const token = form.get("refresh_token") ?? "";
      if (usedRefresh.has(token) || token !== `r${generation}`)
        return Response.json({ error: "invalid_grant" }, { status: 400 });
      usedRefresh.add(token);
      refreshes += 1;
      generation += 1;
      return Response.json({
        access_token: `a${generation}`,
        refresh_token: `r${generation}`,
        expires_in: 3600,
        scope: "orders:read",
        store: "store_1",
        issuer: API,
      });
    }
    const authorization = request.headers.get("authorization");
    calls.push({ url: request.url, authorization, body: await request.text() });
    if (authorization !== `Bearer a${generation}`)
      return Response.json({ code: "invalid_token", error: "no" }, { status: 401 });
    return Response.json({ ok: true });
  };
  return {
    fetch,
    calls,
    get refreshes() {
      return refreshes;
    },
  };
}

const fresh = {
  accessToken: "a1",
  refreshToken: "r1",
  expiresAt: 10_000_000,
  scopes: [],
  store: "store_1",
  issuer: API,
};

test("sends the stored access token", async () => {
  const api = fakeApi();
  const client = new HyphaClient({
    apiUrl: API,
    manifestUrl: MANIFEST,
    tokens: memoryTokenStore(fresh),
    fetch: api.fetch,
    now: () => 0,
  });
  const response = await client.fetch("/admin/orders");
  assert.equal(response.status, 200);
  assert.equal(api.calls[0].authorization, "Bearer a1");
  assert.equal(api.calls[0].url, `${API}/admin/orders`);
  assert.equal(api.refreshes, 0);
});

test("refreshes an expiring token first and stores the rotated pair", async () => {
  const api = fakeApi();
  const store = memoryTokenStore({ ...fresh, expiresAt: 30_000 });
  const client = new HyphaClient({
    apiUrl: API,
    manifestUrl: MANIFEST,
    tokens: store,
    fetch: api.fetch,
    now: () => 0,
  });
  await client.fetch("/admin/orders");
  assert.equal(api.refreshes, 1);
  assert.equal(api.calls[0].authorization, "Bearer a2");
  assert.equal((await store.get())?.refreshToken, "r2");
});

test("concurrent calls share one refresh (a reused refresh token would revoke the grant)", async () => {
  const api = fakeApi();
  const client = new HyphaClient({
    apiUrl: API,
    manifestUrl: MANIFEST,
    tokens: memoryTokenStore({ ...fresh, expiresAt: 0 }),
    fetch: api.fetch,
    now: () => 0,
  });
  const answers = await Promise.all([1, 2, 3].map(() => client.fetch("/admin/orders")));
  assert.deepEqual(
    answers.map((r) => r.status),
    [200, 200, 200],
  );
  assert.equal(api.refreshes, 1);
});

test("a 401 refreshes once and retries with the same body", async () => {
  const api = fakeApi();
  // The store's token is stale although it hasn't expired by the clock.
  const store = memoryTokenStore({ ...fresh, accessToken: "a0" });
  const client = new HyphaClient({
    apiUrl: API,
    manifestUrl: MANIFEST,
    tokens: store,
    fetch: api.fetch,
    now: () => 0,
  });
  const response = await client.fetch("/admin/products", { method: "POST", body: '{"x":1}' });
  assert.equal(response.status, 200);
  assert.deepEqual(
    api.calls.map((c) => [c.authorization, c.body]),
    [
      ["Bearer a0", '{"x":1}'],
      ["Bearer a2", '{"x":1}'],
    ],
  );
});

test("the typed admin client goes through the same auth", async () => {
  const api = fakeApi();
  const client = new HyphaClient({
    apiUrl: API,
    manifestUrl: MANIFEST,
    tokens: memoryTokenStore(fresh),
    fetch: api.fetch,
    now: () => 0,
  });
  const { response } = await client.admin.GET("/admin/products", {});
  assert.equal(response.status, 200);
  assert.equal(api.calls[0].authorization, "Bearer a1");
});

test("no tokens is NotInstalledError", async () => {
  const client = new HyphaClient({
    apiUrl: API,
    manifestUrl: MANIFEST,
    tokens: memoryTokenStore(),
  });
  await assert.rejects(client.fetch("/admin/orders"), NotInstalledError);
});

/** `store`, noting each lock taken in `locks`. */
function recordingLocks(store: TokenStore, locks: string[]): TokenStore {
  return {
    get: () => store.get(),
    set: (tokens) => store.set(tokens),
    withLock: (key, fn) => {
      locks.push(key);
      return store.withLock ? store.withLock(key, fn) : fn();
    },
  };
}

test("two clients sharing a locked store refresh once: the second uses the pair the first stored", async () => {
  const api = fakeApi();
  const locks: string[] = [];
  const locked = recordingLocks(memoryTokenStore({ ...fresh, expiresAt: 0 }), locks);
  const options = { apiUrl: API, manifestUrl: MANIFEST, tokens: locked, fetch: api.fetch };
  // Two processes' clients: no shared single-flight, only the store's lock.
  const first = new HyphaClient({ ...options, now: () => 0 });
  const second = new HyphaClient({ ...options, now: () => 0 });
  const answers = await Promise.all([first.fetch("/admin/orders"), second.fetch("/admin/orders")]);
  assert.deepEqual(
    answers.map((r) => r.status),
    [200, 200],
  );
  assert.equal(api.refreshes, 1);
  assert.deepEqual(locks, [REFRESH_LOCK, REFRESH_LOCK]);
});

test("a 401's refresh runs under the lock too", async () => {
  const api = fakeApi();
  const locks: string[] = [];
  const client = new HyphaClient({
    apiUrl: API,
    manifestUrl: MANIFEST,
    tokens: recordingLocks(memoryTokenStore({ ...fresh, accessToken: "a0" }), locks),
    fetch: api.fetch,
    now: () => 0,
  });
  assert.equal((await client.fetch("/admin/orders")).status, 200);
  assert.deepEqual(locks, [REFRESH_LOCK]);
});

test("a failed refresh is tried again by the next call", async () => {
  const api = fakeApi();
  let failing = true;
  const fetch: typeof globalThis.fetch = async (input, init) => {
    const request = new Request(input, init);
    if (failing && new URL(request.url).pathname === "/oauth/token")
      return Response.json({ error: "temporarily_unavailable" }, { status: 503 });
    return api.fetch(request);
  };
  const client = new HyphaClient({
    apiUrl: API,
    manifestUrl: MANIFEST,
    tokens: memoryTokenStore({ ...fresh, expiresAt: 30_000 }),
    fetch,
    now: () => 0,
  });
  const both = await Promise.allSettled([
    client.fetch("/admin/orders"),
    client.fetch("/admin/orders"),
  ]);
  assert.deepEqual(
    both.map((r) => r.status),
    ["rejected", "rejected"],
  );
  failing = false;
  assert.equal((await client.fetch("/admin/orders")).status, 200);
  assert.equal(api.refreshes, 1);
});
