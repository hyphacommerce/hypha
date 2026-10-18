import assert from "node:assert/strict";
import { test } from "node:test";

import {
  OAuthError,
  codeChallenge,
  createAuthorizationUrl,
  exchangeCode,
  parseAuthorizationResponse,
  refresh,
  revoke,
} from "./oauth.ts";

const API = "https://api.shop.example";
const MANIFEST = "https://app.example/hypha-app.json";

/** A fetch that records requests and answers with `answer`. */
function fakeFetch(answer: (request: Request) => Response) {
  const seen: Request[] = [];
  const fetch: typeof globalThis.fetch = async (input, init) => {
    const request = new Request(input, init);
    seen.push(request.clone());
    return answer(request);
  };
  return { fetch, seen };
}

test("S256 is base64url of the verifier's SHA-256, unpadded", async () => {
  // `printf 'dBjftJeZ4CVP-mJ92K1uzqqLsnCPZHn1qtTVK8Z8YAE' | openssl dgst -sha256 -binary | base64`
  assert.equal(
    await codeChallenge("dBjftJeZ4CVP-mJ92K1uzqqLsnCPZHn1qtTVK8Z8YAE"),
    "ntRAJjilJJ8p6DMS6Oi6I6v8h9hni45cY2XAqFgWtkc",
  );
});

test("the authorization URL carries the client, redirect, scopes, state and challenge", async () => {
  const { url, codeVerifier, state } = await createAuthorizationUrl({
    apiUrl: API,
    manifestUrl: MANIFEST,
    redirectUri: "https://app.example/callback",
    scopes: ["orders:read", "customer_data:email"],
    state: "s1",
  });
  const parsed = new URL(url);
  assert.equal(parsed.origin + parsed.pathname, `${API}/oauth/authorize`);
  const q = parsed.searchParams;
  assert.equal(q.get("response_type"), "code");
  assert.equal(q.get("client_id"), MANIFEST);
  assert.equal(q.get("redirect_uri"), "https://app.example/callback");
  assert.equal(q.get("scope"), "orders:read customer_data:email");
  assert.equal(q.get("state"), "s1");
  assert.equal(state, "s1");
  assert.equal(q.get("code_challenge_method"), "S256");
  assert.equal(q.get("code_challenge"), await codeChallenge(codeVerifier));
  assert.match(codeVerifier, /^[A-Za-z0-9_-]{43}$/);
});

test("a state is made when none is given, and verifiers differ", async () => {
  const input = { apiUrl: API, manifestUrl: MANIFEST, redirectUri: "https://a/cb", scopes: [] };
  const a = await createAuthorizationUrl(input);
  const b = await createAuthorizationUrl(input);
  assert.ok(a.state.length > 0);
  assert.notEqual(a.state, b.state);
  assert.notEqual(a.codeVerifier, b.codeVerifier);
});

test("parses the redirect's code or refusal", () => {
  assert.deepEqual(parseAuthorizationResponse("https://a/cb?code=hypha_ac_1&state=s"), {
    ok: true,
    code: "hypha_ac_1",
    state: "s",
  });
  assert.deepEqual(parseAuthorizationResponse("https://a/cb?error=access_denied&state=s"), {
    ok: false,
    error: "access_denied",
    state: "s",
  });
  assert.deepEqual(parseAuthorizationResponse("https://a/cb?code=x"), {
    ok: false,
    error: "invalid_request",
  });
});

test("exchanging a code posts the form and returns tokens and the app secret", async () => {
  const { fetch, seen } = fakeFetch(() =>
    Response.json({
      access_token: "hypha_at_1",
      token_type: "Bearer",
      expires_in: 3600,
      refresh_token: "hypha_rt_1",
      scope: "orders:read customer_data:email",
      app_secret: "hypha_shs_x",
    }),
  );
  const tokens = await exchangeCode({
    apiUrl: API,
    manifestUrl: MANIFEST,
    code: "hypha_ac_1",
    codeVerifier: "v",
    redirectUri: "https://app.example/callback",
    fetch,
    now: () => 1_000,
  });
  assert.deepEqual(tokens, {
    accessToken: "hypha_at_1",
    refreshToken: "hypha_rt_1",
    expiresAt: 1_000 + 3_600_000,
    scopes: ["orders:read", "customer_data:email"],
    appSecret: "hypha_shs_x",
  });
  const [request] = seen;
  assert.ok(request);
  assert.equal(request.url, `${API}/oauth/token`);
  assert.equal(request.headers.get("content-type"), "application/x-www-form-urlencoded");
  const form = new URLSearchParams(await request.text());
  assert.deepEqual(Object.fromEntries(form), {
    grant_type: "authorization_code",
    code: "hypha_ac_1",
    redirect_uri: "https://app.example/callback",
    code_verifier: "v",
    client_id: MANIFEST,
  });
});

test("refreshing returns the rotated refresh token", async () => {
  const { fetch, seen } = fakeFetch(() =>
    Response.json({ access_token: "a2", refresh_token: "r2", expires_in: 60, scope: "" }),
  );
  const tokens = await refresh({
    apiUrl: API,
    manifestUrl: MANIFEST,
    refreshToken: "r1",
    fetch,
    now: () => 0,
  });
  assert.deepEqual(tokens, {
    accessToken: "a2",
    refreshToken: "r2",
    expiresAt: 60_000,
    scopes: [],
  });
  const form = new URLSearchParams(await seen[0].text());
  assert.equal(form.get("grant_type"), "refresh_token");
  assert.equal(form.get("refresh_token"), "r1");
});

test("RFC 6749 errors become OAuthError", async () => {
  const { fetch } = fakeFetch(() =>
    Response.json({ error: "invalid_grant", error_description: "reused" }, { status: 400 }),
  );
  await assert.rejects(
    refresh({ apiUrl: API, manifestUrl: MANIFEST, refreshToken: "r1", fetch }),
    (error: unknown) =>
      error instanceof OAuthError &&
      error.code === "invalid_grant" &&
      error.description === "reused" &&
      error.status === 400,
  );
});

test("an answer without tokens is invalid_response", async () => {
  const { fetch } = fakeFetch(() => Response.json({ token_type: "Bearer" }));
  await assert.rejects(
    refresh({ apiUrl: API, manifestUrl: MANIFEST, refreshToken: "r1", fetch }),
    (error: unknown) => error instanceof OAuthError && error.code === "invalid_response",
  );
});

test("revoke posts the token", async () => {
  const { fetch, seen } = fakeFetch(() => new Response(null, { status: 200 }));
  await revoke({ apiUrl: API, manifestUrl: MANIFEST, token: "hypha_rt_1", fetch });
  assert.equal(seen[0].url, `${API}/oauth/revoke`);
  assert.equal(new URLSearchParams(await seen[0].text()).get("token"), "hypha_rt_1");
});
