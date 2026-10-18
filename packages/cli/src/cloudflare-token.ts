// The Cloudflare API token a deployment runs on (design note 0026, §5): one token, kept
// in the deployment's `.env`, for wrangler (the API Worker, Hyperdrive, KV, R2) and `cf`
// (the admin and storefront Workers), so neither tool's own login matters and everything
// lands in the token's account. The CLI can't create the token (no login may create
// tokens), so `hypha init` opens Cloudflare's form filled in with what Hypha needs, and
// checks the token it's given against the account and the domain.

import { createHash } from "node:crypto";

/** Why an R2 key pair can't be R2's S3 credentials, if it can't: R2 shows a token's
 * Access Key ID (32 hex characters) and Secret Access Key (64) apart from the token's
 * value (`cfat_…`), which is what wrangler-style tools take, not the S3 API. */
export function checkR2Keys(id: string, secret: string): string | undefined {
  if (/^cf[a-z]*_/.test(id) || /^cf[a-z]*_/.test(secret)) {
    return "that's the token's value; R2 shows the token's Access Key ID and Secret Access Key below it";
  }
  if (!/^[0-9a-f]{32}$/i.test(id)) return "the Access Key ID is 32 hexadecimal characters";
  if (!/^[0-9a-f]{64}$/i.test(secret)) return "the Secret Access Key is 64 hexadecimal characters";
  return undefined;
}

/**
 * R2's S3 credentials from an API token with R2 permission (developers.cloudflare.com/r2/
 * api/tokens): the Access Key ID is the token's ID, the Secret Access Key the SHA-256 of
 * its value. So the deploy token can give the media's keys, and a pasted token value is
 * turned into them.
 */
export async function r2KeysOf(
  token: string,
  accountId: string,
  get: typeof fetch = fetch,
): Promise<{ id: string; secret: string }> {
  // A user token verifies under /user, an account-owned one (`cfat_…`) under its account.
  const verified =
    (await call<{ id: string }>(get, token, "/user/tokens/verify")) ??
    (await call<{ id: string }>(get, token, `/accounts/${accountId}/tokens/verify`));
  if (!verified?.id) throw new Error("that R2 token isn't valid (or has expired)");
  const secret = createHash("sha256").update(token).digest("hex");
  return { id: verified.id, secret };
}

/** Whether `value` is a Cloudflare API token's value rather than an S3 key. */
export function isTokenValue(value: string): boolean {
  return /^cf[a-z]*_/.test(value) || (value.length >= 40 && !/^[0-9a-f]+$/i.test(value));
}

/** What the token must allow, as the dashboard's template keys
 * (developers.cloudflare.com/fundamentals/api/how-to/account-owned-token-template). The
 * template has no key for some (`manual`: the form's own names), which the person adds
 * to the filled-in form themselves. */
export const PERMISSIONS: { key: string; type: "read" | "edit"; why: string; manual?: string }[] = [
  { key: "account_settings", type: "read", why: "finding the account" },
  { key: "workers_scripts", type: "edit", why: "deploying the three Workers" },
  { key: "workers_tail", type: "read", why: "`hypha logs`", manual: "Account › Workers Tail › Read" },
  { key: "workers_kv_storage", type: "edit", why: "the jobs' marks and the storefront's cache" },
  { key: "workers_r2", type: "edit", why: "the media bucket" },
  { key: "hyperdrive", type: "edit", why: "the database connection", manual: "Account › Hyperdrive › Edit" },
  { key: "zone", type: "read", why: "finding the domain" },
  { key: "workers_routes", type: "edit", why: "the Workers' custom domains" },
  { key: "dns", type: "edit", why: "the custom domains' records" },
];

/** The permissions the link can't fill in, as the form names them. */
export const MANUAL = PERMISSIONS.flatMap((p) => (p.manual ? [p.manual] : []));

/** The dashboard's create-token page, filled in with `PERMISSIONS` (except `MANUAL`). */
export function templateUrl(name = "Hypha deploy"): string {
  const filled = PERMISSIONS.filter((p) => !p.manual).map(({ key, type }) => ({ key, type }));
  const keys = encodeURIComponent(JSON.stringify(filled));
  return (
    "https://dash.cloudflare.com/profile/api-tokens" +
    `?permissionGroupKeys=${keys}&accountId=*&zoneId=all&name=${encodeURIComponent(name)}`
  );
}

const API = "https://api.cloudflare.com/client/v4";

type Envelope<T> = { success: boolean; result: T; errors?: { code: number; message: string }[] };

/** A call to Cloudflare's API with `token`: its result, or `null` when it's refused. */
async function call<T>(get: typeof fetch, token: string, path: string): Promise<T | null> {
  const response = await get(`${API}${path}`, { headers: { authorization: `Bearer ${token}` } });
  if (response.status === 401 || response.status === 403) return null;
  const body = (await response.json().catch(() => null)) as Envelope<T> | null;
  if (!body) throw new Error(`Cloudflare's API answered ${path} with ${response.status}`);
  if (!body.success) return null;
  return body.result;
}

export type Checked = {
  accountId: string;
  zoneId: string;
  /** What the token lacks, each with why it's needed; empty when it's enough. */
  missing: string[];
  /** Whether it may use R2 (its S3 keys can then be derived from it, `r2KeysOf`). */
  r2: boolean;
};

/**
 * Checks `token`: that it's valid, which account it reaches (`accountId` when given, else
 * the only one), that the account holds `domain`'s zone, and that each permission
 * deploying needs answers (a read of each kind of resource; `media` adds R2).
 */
export async function checkToken(
  token: string,
  domain: string,
  options: { accountId?: string; zoneId?: string; /** R2 is required, not just tried. */ media: boolean },
  get: typeof fetch = fetch,
): Promise<Checked> {
  const verified = await call<{ status: string }>(get, token, "/user/tokens/verify");
  if (!verified || verified.status !== "active") {
    throw new Error("the API token isn't valid (or has expired): create one with the link, then paste it");
  }
  const accounts = (await call<{ id: string; name: string }[]>(get, token, "/accounts")) ?? [];
  let accountId = options.accountId;
  if (!accountId) {
    if (accounts.length !== 1) {
      throw new Error(
        `the token reaches ${accounts.length} accounts: pass --account-id` +
          (accounts.length ? `\n${accounts.map((a) => `${a.id}  ${a.name}`).join("\n")}` : ""),
      );
    }
    accountId = accounts[0].id;
  } else if (accounts.length > 0 && !accounts.some((a) => a.id === accountId)) {
    throw new Error(`the token doesn't reach account ${accountId}`);
  }
  // The zone is the domain or one of its parents: shop.example.co.uk is in example.co.uk.
  const labels = domain.split(".");
  let zoneId = options.zoneId;
  for (let i = 0; i < labels.length - 1 && !zoneId; i++) {
    const name = labels.slice(i).join(".");
    const zones = await call<{ id: string }[]>(get, token, `/zones?name=${encodeURIComponent(name)}&account.id=${accountId}`);
    zoneId = zones?.[0]?.id;
  }
  if (!zoneId) {
    throw new Error(`no zone for ${domain} in that account (or the token can't read zones): add the domain to Cloudflare first`);
  }
  const probes: [string, string][] = [
    [`/accounts/${accountId}/workers/scripts`, "workers_scripts"],
    [`/accounts/${accountId}/storage/kv/namespaces?per_page=5`, "workers_kv_storage"],
    [`/accounts/${accountId}/hyperdrive/configs`, "hyperdrive"],
    [`/zones/${zoneId}/workers/routes`, "workers_routes"],
    [`/zones/${zoneId}/dns_records?per_page=1`, "dns"],
  ];
  const lacking = (key: string) => {
    const permission = PERMISSIONS.find((p) => p.key === key)!;
    return `${permission.manual ?? `${key} (${permission.type})`}: ${permission.why}`;
  };
  const missing: string[] = [];
  for (const [path, key] of probes) {
    if ((await call<unknown>(get, token, path)) === null) missing.push(lacking(key));
  }
  const r2 = (await call<unknown>(get, token, `/accounts/${accountId}/r2/buckets`)) !== null;
  if (options.media && !r2) missing.push(lacking("workers_r2"));
  return { accountId, zoneId, missing, r2 };
}
