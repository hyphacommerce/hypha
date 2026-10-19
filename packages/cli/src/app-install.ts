// Installing the co-located apps (design note 0029, §9 and §4.1): the deploy runs them,
// but only a staff member installs one, on its consent page, like any app's. For each
// app the store hasn't installed, `hypha deploy` (and `hypha apps install`) prints and
// opens `<app_url>/install?api=<the API's public URL>`, the convention every admin's
// "Install app" uses; `hypha apps status` says which are.

import { getJson, Staff } from "./api.ts";
import { type AppEntry, appUrl } from "./apps.ts";
import { type Deployment, type EnvFile, url } from "./deployment.ts";

/** What reading the store's apps needs of a staff session (a fake one in tests). */
export type Calls = Pick<Staff, "call">;

/** The link that starts installing `entry` into this deployment's store. */
export function installLink(d: Deployment, entry: AppEntry): string {
  return `${appUrl(d, entry)}/install?api=${encodeURIComponent(url(d, d.domains.api))}`;
}

/** The key the app's manifest gives it (`app_stripe`); `app_<name>` when it can't be read. */
export async function appKey(
  d: Deployment,
  entry: AppEntry,
  fetchJson: (target: string) => Promise<unknown> = (target) => getJson(d, target),
): Promise<{ key: string; reachable: boolean }> {
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the app's manifest; only `key` is read, and checked
  const manifest = (await fetchJson(`${appUrl(d, entry)}/manifest.json`)) as
    | { key?: unknown }
    | undefined;
  return typeof manifest?.key === "string"
    ? { key: manifest.key, reachable: true }
    : { key: `app_${entry.name.replaceAll("-", "_")}`, reachable: false };
}

type AppSummary = { key: string; installed: boolean; enabled: boolean };

/** Every app in the store, by key (all pages of `GET /admin/apps`). */
export async function storeApps(staff: Calls): Promise<Map<string, AppSummary>> {
  const out = new Map<string, AppSummary>();
  let cursor: string | null = null;
  do {
    const query: string = cursor ? `&cursor=${encodeURIComponent(cursor)}` : "";
    const page: { items: AppSummary[]; next_cursor: string | null } = await staff.call(
      "GET",
      `/admin/apps?limit=100${query}`,
    );
    for (const app of page.items) out.set(app.key, app);
    cursor = page.next_cursor;
  } while (cursor);
  return out;
}

export type AppStatus = {
  entry: AppEntry;
  key: string;
  url: string;
  /** Its manifest answered. */
  reachable: boolean;
  /** Installed in the store through OAuth (with a grant in force); absent when the
   * store couldn't be read. */
  installed?: boolean;
  link: string;
};

/** Each co-located app: where it is, whether it answers, whether the store has it. */
export async function appStatuses(
  d: Deployment,
  staff: Calls | undefined,
  fetchJson?: (target: string) => Promise<unknown>,
): Promise<AppStatus[]> {
  const installed = staff ? await storeApps(staff) : undefined;
  const out: AppStatus[] = [];
  for (const entry of d.apps ?? []) {
    const { key, reachable } = await appKey(d, entry, fetchJson);
    out.push({
      entry,
      key,
      url: appUrl(d, entry),
      reachable,
      ...(installed ? { installed: installed.get(key)?.installed ?? false } : {}),
      link: installLink(d, entry),
    });
  }
  return out;
}

/**
 * A staff session for reading the store's apps: `HYPHA_ADMIN_KEY` (a secret key with
 * `apps:read`, from this shell or `.env`), else the owner's login from `.env`. None when
 * neither is there, or the login doesn't go through (two-factor on, a changed password).
 */
export async function staffFor(
  d: Deployment,
  env: EnvFile | undefined,
): Promise<Staff | undefined> {
  const key = process.env.HYPHA_ADMIN_KEY || env?.get("HYPHA_ADMIN_KEY");
  if (key) return Staff.withToken(d, key);
  const email = env?.get("HYPHA_OWNER_EMAIL");
  const password = env?.get("HYPHA_OWNER_PASSWORD");
  if (!email || !password) return undefined;
  return Staff.login(d, email, password).catch(() => undefined);
}
