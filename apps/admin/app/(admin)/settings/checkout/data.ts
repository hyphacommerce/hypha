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
