import assert from "node:assert/strict";
import { test } from "node:test";

import {
  checkR2Keys,
  checkToken,
  isTokenValue,
  PERMISSIONS,
  r2KeysOf,
  templateUrl,
} from "./cloudflare-token.ts";

test("the link fills in every permission deploying needs", () => {
  const link = new URL(templateUrl("Hypha shop"));
  assert.equal(link.origin + link.pathname, "https://dash.cloudflare.com/profile/api-tokens");
  assert.equal(link.searchParams.get("name"), "Hypha shop");
  assert.deepEqual(
    JSON.parse(link.searchParams.get("permissionGroupKeys")!),
    PERMISSIONS.map(({ key, type }) => ({ key, type })),
  );
});

/** The URL a fake `fetch` was called with. */
function urlOf(input: RequestInfo | URL): string {
  return input instanceof Request ? input.url : input.toString();
}

/** A successful answer from Cloudflare's API. */
function answer(result: unknown): Response {
  return new Response(JSON.stringify({ success: true, result }), { status: 200 });
}

/** Cloudflare's API as a fake: `refused` paths answer 403. */
function cloudflare(
  options: { accounts?: { id: string; name: string }[]; refused?: string[]; zone?: string } = {},
) {
  const asked: string[] = [];
  const get = (async (input: RequestInfo | URL) => {
    const path = urlOf(input).replace("https://api.cloudflare.com/client/v4", "");
    asked.push(path);
    if ((options.refused ?? []).some((r) => path.startsWith(r)))
      return new Response("{}", { status: 403 });
    if (path === "/user/tokens/verify") return answer({ status: "active" });
    if (path === "/accounts") return answer(options.accounts ?? [{ id: "acc", name: "Shop" }]);
    if (path.startsWith("/zones?name=")) {
      const name = decodeURIComponent(path.split("name=")[1].split("&")[0]);
      return answer(name === (options.zone ?? "example.com") ? [{ id: "zone1" }] : []);
    }
    return answer([]);
  }) as typeof fetch;
  return { get, asked };
}

test("a good token gives the account and the domain's zone, its parent's for a subdomain", async () => {
  const { get } = cloudflare();
  assert.deepEqual(await checkToken("t", "shop.example.com", { media: true }, get), {
    accountId: "acc",
    zoneId: "zone1",
    missing: [],
    r2: true,
  });
});

test("a token that can't reach something says which permission to add", async () => {
  const { get } = cloudflare({ refused: ["/accounts/acc/hyperdrive", "/zones/zone1/dns_records"] });
  const { missing } = await checkToken("t", "example.com", { media: false }, get);
  assert.deepEqual(
    missing.map((m) => m.split(" ")[0]),
    ["Hyperdrive", "DNS"],
  );
});

test("R2 is tried, and required only when asked", async () => {
  const { get } = cloudflare({ refused: ["/accounts/acc/r2"] });
  const tried = await checkToken("t", "example.com", { media: false }, get);
  assert.deepEqual([tried.missing, tried.r2], [[], false]);
  assert.equal((await checkToken("t", "example.com", { media: true }, get)).missing.length, 1);
  assert.equal((await checkToken("t", "example.com", { media: false }, cloudflare().get)).r2, true);
});

test("D1 is required only when apps are deployed alongside", async () => {
  const { get, asked } = cloudflare({ refused: ["/accounts/acc/d1"] });
  const without = await checkToken("t", "example.com", { media: false }, get);
  assert.deepEqual(without.missing, []);
  assert.ok(!asked.some((path) => path.includes("/d1/")));
  const { missing } = await checkToken("t", "example.com", { media: false, apps: true }, get);
  assert.deepEqual(
    missing.map((m) => m.split(" (")[0]),
    ["D1"],
  );
});

test("R2's keys from a token: its ID and the SHA-256 of its value, user or account token", async () => {
  const user = (async (input: RequestInfo | URL) =>
    urlOf(input).endsWith("/user/tokens/verify")
      ? new Response(
          JSON.stringify({ success: true, result: { id: "f".repeat(32), status: "active" } }),
        )
      : new Response("{}", { status: 403 })) as typeof fetch;
  assert.deepEqual(await r2KeysOf("abc", "acc", user), {
    id: "f".repeat(32),
    secret: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
  });
  const account = (async (input: RequestInfo | URL) =>
    urlOf(input).endsWith("/accounts/acc/tokens/verify")
      ? new Response(
          JSON.stringify({ success: true, result: { id: "e".repeat(32), status: "active" } }),
        )
      : new Response("{}", { status: 401 })) as typeof fetch;
  assert.equal((await r2KeysOf("cfat_x", "acc", account)).id, "e".repeat(32));
  assert.ok(isTokenValue("cfat_EXAMPLE_not_a_real_token_0000000000000000000"));
  assert.ok(!isTokenValue("0".repeat(32)));
});

test("several accounts need --account-id; a domain elsewhere is refused", async () => {
  const two = cloudflare({
    accounts: [
      { id: "a1", name: "One" },
      { id: "a2", name: "Two" },
    ],
  });
  await assert.rejects(
    checkToken("t", "example.com", { media: false }, two.get),
    /reaches 2 accounts/,
  );
  assert.equal(
    (await checkToken("t", "example.com", { accountId: "a2", media: false }, two.get)).accountId,
    "a2",
  );
  const elsewhere = cloudflare({ zone: "other.com" });
  await assert.rejects(
    checkToken("t", "example.com", { media: false }, elsewhere.get),
    /no zone for example\.com/,
  );
});

test("an invalid token is refused before anything else", async () => {
  const { get } = cloudflare({ refused: ["/user/tokens/verify"] });
  await assert.rejects(checkToken("t", "example.com", { media: false }, get), /isn't valid/);
});

test("R2's S3 keys are told apart from a token's value", () => {
  assert.equal(checkR2Keys("0".repeat(32), "f".repeat(64)), undefined);
  assert.match(
    checkR2Keys("cfat_EXAMPLE_not_a_real_token_0000000000000000000", "x") ?? "",
    /token's value/,
  );
  assert.match(checkR2Keys("0".repeat(31), "f".repeat(64)) ?? "", /32 hexadecimal/);
  assert.match(checkR2Keys("0".repeat(32), "f".repeat(63)) ?? "", /64 hexadecimal/);
});

test("a zone given by flag is used as it is", async () => {
  const { get, asked } = cloudflare({ zone: "elsewhere.com" });
  const checked = await checkToken("t", "example.com", { zoneId: "zflag", media: false }, get);
  assert.equal(checked.zoneId, "zflag");
  assert.ok(!asked.some((p) => p.startsWith("/zones?")));
});
