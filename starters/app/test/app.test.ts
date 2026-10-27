import assert from "node:assert/strict";
import { test } from "node:test";

import { signRequest } from "@hyphacommerce/app";

import { createApp } from "../src/app.ts";
import { MemoryInstallations } from "../src/installations.ts";

const APP_URL = "https://app.example";
const STORE = "store_0190f1a2b3c47d8e9f00112233445566";
const SECRET = "hypha_shs_test";

async function appWithInstallation() {
  const installations = new MemoryInstallations();
  await installations.save({
    store: STORE,
    apiUrl: "https://api.example",
    secret: SECRET,
    tokens: {
      accessToken: "hypha_at_x",
      refreshToken: "hypha_rt_x",
      expiresAt: Date.now() + 3_600_000,
      scopes: ["shipping:read"],
      store: STORE,
      issuer: "https://api.example",
    },
  });
  return createApp({ appUrl: APP_URL, installations });
}

test("the manifest is served at its URL, naming the routes", async () => {
  const app = createApp({ appUrl: APP_URL, installations: new MemoryInstallations() });
  const response = await app.request("/manifest.json");
  assert.equal(response.status, 200);
  const manifest: unknown = await response.json();
  assert.partialDeepStrictEqual(manifest, {
    key: "app_starter",
    app_url: APP_URL,
    redirect_uris: ["/hypha/oauth/callback"],
  });
});

test("a signed shipping.rates call is answered with the flat rate", async () => {
  const app = await appWithInstallation();
  const body = JSON.stringify({
    id: "rreq_1",
    sent_at: "2026-10-26T00:00:00Z",
    request: {
      currency: "EUR",
      destination: { country: "DE" },
      items: [],
      subtotal: { amount: 2500, currency: "EUR" },
    },
  });
  const response = await app.request("/hypha/extensions", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "Hypha-Signature": await signRequest(SECRET, body),
      "Hypha-Extension": "shipping.rates",
      "Hypha-Extension-Version": "1",
      "Hypha-Store": STORE,
    },
    body,
  });
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), {
    rates: [
      {
        service: "starter_flat",
        name: "Flat rate",
        amount: { amount: 990, currency: "EUR" },
        estimate: { min_days: 2, max_days: 5 },
      },
    ],
  });
});

test("a call for an unknown store, or with a wrong signature, is 401", async () => {
  const app = await appWithInstallation();
  const body = "{}";
  for (const [store, secret] of [
    ["store_unknown", SECRET],
    [STORE, "wrong"],
  ] as const) {
    const response = await app.request("/hypha/extensions", {
      method: "POST",
      headers: {
        "Hypha-Signature": await signRequest(secret, body),
        "Hypha-Extension": "shipping.rates",
        "Hypha-Store": store,
      },
      body,
    });
    assert.equal(response.status, 401);
  }
});
