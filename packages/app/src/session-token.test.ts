import assert from "node:assert/strict";
import { test } from "node:test";

import { toBase64Url, utf8 } from "./encoding.ts";
import { SessionTokenError, signSessionToken, verifySessionToken } from "./session-token.ts";

const SECRET = "hypha_shs_test";
const claims = {
  iss: "https://api.shop.example",
  aud: "app_stripe",
  sub: "user_1",
  store: "store_1",
  scope: "payments:write orders:read",
  iat: 1_700_000_000,
  exp: 1_700_000_060,
  jti: "j1",
};
const now = 1_700_000_010;

function rejectsWith(code: string) {
  return (error: unknown) => error instanceof SessionTokenError && error.code === code;
}

test("verifies a token and splits its scopes", async () => {
  const token = await signSessionToken(claims, SECRET);
  assert.deepEqual(await verifySessionToken(token, SECRET, { audience: "app_stripe", now }), {
    ...claims,
    scopes: ["payments:write", "orders:read"],
  });
});

test("matches an HS256 JWT made elsewhere", async () => {
  // Built by hand the way any JWT library does: base64url(header).base64url(payload).
  const header = toBase64Url(utf8('{"typ":"JWT","alg":"HS256"}'));
  const payload = toBase64Url(utf8(JSON.stringify(claims)));
  const key = await crypto.subtle.importKey(
    "raw",
    utf8(SECRET),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = new Uint8Array(await crypto.subtle.sign("HMAC", key, utf8(`${header}.${payload}`)));
  const token = `${header}.${payload}.${toBase64Url(mac)}`;
  const verified = await verifySessionToken(token, SECRET, { audience: "app_stripe", now });
  assert.equal(verified.sub, "user_1");
});

test("rejects another secret", async () => {
  const token = await signSessionToken(claims, "other");
  await assert.rejects(
    verifySessionToken(token, SECRET, { audience: "app_stripe", now }),
    rejectsWith("invalid_signature"),
  );
});

test("rejects alg none", async () => {
  const token = await signSessionToken(claims, SECRET);
  const [, payload] = token.split(".");
  const none = `${toBase64Url(utf8('{"alg":"none"}'))}.${payload}.`;
  await assert.rejects(
    verifySessionToken(none, SECRET, { audience: "app_stripe", now }),
    rejectsWith("unsupported_algorithm"),
  );
});

test("expires at exp, give or take the tolerance", async () => {
  const token = await signSessionToken(claims, SECRET);
  const options = { audience: "app_stripe", clockToleranceSeconds: 0 };
  await verifySessionToken(token, SECRET, { ...options, now: claims.exp - 1 });
  await assert.rejects(
    verifySessionToken(token, SECRET, { ...options, now: claims.exp }),
    rejectsWith("expired"),
  );
});

test("rejects a token issued in the future", async () => {
  const token = await signSessionToken(claims, SECRET);
  await assert.rejects(
    verifySessionToken(token, SECRET, { audience: "app_stripe", now: claims.iat - 6 }),
    rejectsWith("not_yet_valid"),
  );
});

test("rejects another app's token and another issuer", async () => {
  const token = await signSessionToken(claims, SECRET);
  await assert.rejects(
    verifySessionToken(token, SECRET, { audience: "app_other", now }),
    rejectsWith("wrong_audience"),
  );
  await assert.rejects(
    verifySessionToken(token, SECRET, { audience: "app_stripe", issuer: "https://x", now }),
    rejectsWith("wrong_issuer"),
  );
});

test("rejects malformed tokens", async () => {
  for (const token of ["", "a.b", "a.b.c.d", "!!.e30.x"]) {
    await assert.rejects(
      verifySessionToken(token, SECRET, { audience: "app_stripe", now }),
      rejectsWith("malformed"),
    );
  }
});
