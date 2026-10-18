import assert from "node:assert/strict";
import { test } from "node:test";

import { checkToken, PERMISSIONS, templateUrl } from "./cloudflare-token.ts";

test("the link fills in every permission deploying needs", () => {
  const link = new URL(templateUrl("Hypha shop"));
  assert.equal(link.origin + link.pathname, "https://dash.cloudflare.com/profile/api-tokens");
  assert.equal(link.searchParams.get("name"), "Hypha shop");
  assert.deepEqual(
    JSON.parse(link.searchParams.get("permissionGroupKeys")!),
    PERMISSIONS.map(({ key, type }) => ({ key, type })),
  );
});

/** Cloudflare's API as a fake: `refused` paths answer 403. */
function cloudflare(options: { accounts?: { id: string; name: string }[]; refused?: string[]; zone?: string } = {}) {
  const asked: string[] = [];
  const get = (async (input: RequestInfo | URL) => {
    const path = String(input).replace("https://api.cloudflare.com/client/v4", "");
    asked.push(path);
    const answer = (result: unknown) => new Response(JSON.stringify({ success: true, result }), { status: 200 });
    if ((options.refused ?? []).some((r) => path.startsWith(r))) return new Response("{}", { status: 403 });
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
  });
});

test("a token that can't reach something says which permission to add", async () => {
  const { get } = cloudflare({ refused: ["/accounts/acc/hyperdrive", "/zones/zone1/dns_records"] });
  const { missing } = await checkToken("t", "example.com", { media: false }, get);
  assert.deepEqual(missing.map((m) => m.split(" ")[0]), ["hyperdrive", "dns"]);
});

test("R2 is only checked when there's media", async () => {
  const { get, asked } = cloudflare({ refused: ["/accounts/acc/r2"] });
  assert.deepEqual((await checkToken("t", "example.com", { media: false }, get)).missing, []);
  assert.ok(!asked.some((p) => p.includes("/r2/")));
  assert.equal((await checkToken("t", "example.com", { media: true }, get)).missing.length, 1);
});

test("several accounts need --account-id; a domain elsewhere is refused", async () => {
  const two = cloudflare({ accounts: [{ id: "a1", name: "One" }, { id: "a2", name: "Two" }] });
  await assert.rejects(checkToken("t", "example.com", { media: false }, two.get), /reaches 2 accounts/);
  assert.equal((await checkToken("t", "example.com", { accountId: "a2", media: false }, two.get)).accountId, "a2");
  const elsewhere = cloudflare({ zone: "other.com" });
  await assert.rejects(checkToken("t", "example.com", { media: false }, elsewhere.get), /no zone for example\.com/);
});

test("an invalid token is refused before anything else", async () => {
  const { get } = cloudflare({ refused: ["/user/tokens/verify"] });
  await assert.rejects(checkToken("t", "example.com", { media: false }, get), /isn't valid/);
});
