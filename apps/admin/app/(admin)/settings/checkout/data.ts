// Server-only: what the checkout rules pages read besides the rules, for the editor's
// pickers and the list's sentences.

import { flatten } from "@/lib/categories";
import type { RuleOptions } from "@/lib/checkout-rules";
import { loadCountries, type sessionClient } from "@/lib/hypha";
import { countryOptions } from "@/lib/regions";

type Client = Awaited<ReturnType<typeof sessionClient>>;

/**
 * The pickers' choices. A list the role can't read is empty: the API still checks
 * nothing it doesn't know, and the rule shows IDs for those.
 */
export async function loadRuleOptions(client: Client): Promise<RuleOptions> {
  const [countries, markets, channels, zones, categories] = await Promise.all([
    loadCountries().catch(() => []),
    client.GET("/admin/markets"),
    client.GET("/admin/channels"),
    client.GET("/admin/shipping-zones"),
    client.GET("/admin/categories"),
  ]);
  return {
    countries: countryOptions(countries),
    markets: (markets.data?.items ?? []).map((m) => ({
      value: m.id,
      label: m.name,
      currency: m.currency,
    })),
    channels: (channels.data?.items ?? []).map((c) => ({ value: c.id, label: c.name })),
    shippingOptions: (zones.data?.items ?? []).flatMap((zone) =>
      zone.options.map((o) => ({ value: o.id, label: `${zone.name} › ${o.name}` })),
    ),
    categories: flatten(categories.data?.items ?? []).map((row) => ({
      value: row.id,
      label: row.path,
    })),
  };
}

/**
 * The installed apps that answer `address.verify`, for the verification picker: an app's
 * endpoints are read when its list entry doesn't rule it out. None without `apps:read`.
 */
export async function loadVerificationApps(
  client: Client,
): Promise<{ value: string; label: string }[]> {
  const apps = await client.GET("/admin/apps");
  const candidates = (apps.data?.items ?? []).filter((a) => a.enabled);
  const answering = await Promise.all(
    candidates.map(async (app) => {
      const points = await client.GET("/admin/apps/{key}/extension-points", {
        params: { path: { key: app.key } },
      });
      const answers = (points.data?.extension_points ?? []).some(
        (p) => p.point === "address.verify",
      );
      return answers ? { value: app.key, label: app.name } : null;
    }),
  );
  return answering.filter((a) => a !== null);
}
