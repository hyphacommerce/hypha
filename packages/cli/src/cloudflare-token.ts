// The Cloudflare API token a deployment runs on (design note 0026, §5): one token, kept
// in the deployment's `.env`, for wrangler (the API Worker, Hyperdrive, KV, R2) and `cf`
// (the admin and storefront Workers), so neither tool's own login matters and everything
// lands in the token's account. The CLI can't create the token (no login may create
// tokens), so `hypha init` opens Cloudflare's form filled in with what Hypha needs, and
// checks the token it's given against the account and the domain.

/** What the token must allow, as the dashboard's template keys
 * (developers.cloudflare.com/fundamentals/api/how-to/account-owned-token-template). */
export const PERMISSIONS: { key: string; type: "read" | "edit"; why: string }[] = [
  { key: "account_settings", type: "read", why: "finding the account" },
  { key: "workers_scripts", type: "edit", why: "deploying the three Workers" },
  { key: "workers_tail", type: "read", why: "`hypha logs`" },
  { key: "workers_kv_storage", type: "edit", why: "the jobs' marks and the storefront's cache" },
  { key: "workers_r2", type: "edit", why: "the media bucket" },
  { key: "hyperdrive", type: "edit", why: "the database connection" },
  { key: "zone", type: "read", why: "finding the domain" },
  { key: "workers_routes", type: "edit", why: "the Workers' custom domains" },
  { key: "dns", type: "edit", why: "the custom domains' records" },
];

/** The dashboard's create-token page, filled in with `PERMISSIONS`. */
export function templateUrl(name = "Hypha deploy"): string {
  const keys = encodeURIComponent(JSON.stringify(PERMISSIONS.map(({ key, type }) => ({ key, type }))));
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
};

/**
 * Checks `token`: that it's valid, which account it reaches (`accountId` when given, else
 * the only one), that the account holds `domain`'s zone, and that each permission
 * deploying needs answers (a read of each kind of resource; `media` adds R2).
 */
export async function checkToken(
  token: string,
  domain: string,
  options: { accountId?: string; media: boolean },
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
  let zoneId: string | undefined;
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
    ...(options.media ? ([[`/accounts/${accountId}/r2/buckets`, "workers_r2"]] as [string, string][]) : []),
  ];
  const missing: string[] = [];
  for (const [path, key] of probes) {
    if ((await call<unknown>(get, token, path)) === null) {
      const permission = PERMISSIONS.find((p) => p.key === key)!;
      missing.push(`${key} (${permission.type}): ${permission.why}`);
    }
  }
  return { accountId, zoneId, missing };
}
