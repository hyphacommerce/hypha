import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { loaded, sessionClient } from "@/lib/hypha";

import { ProvidersCard, type RateApp } from "./providers";
import { ZonesSection } from "./zones";

/** Settings → Shipping (design note 0015, section 8): where the store ships, what each
 * zone offers, and the rate apps that quote live prices. */
export default async function ShippingSettingsPage() {
  const client = await sessionClient();
  const [zones, providers, apps] = await Promise.all([
    client.GET("/admin/shipping-zones").then(loaded),
    client.GET("/admin/shipping-providers").then(loaded),
    client.GET("/admin/apps").then(loaded),
  ]);
  // A rate app's name and switch are its app's (design note 0016, EV11).
  const byKey = new Map(apps.items.map((app) => [app.key, app]));
  const rateApps: RateApp[] = providers.items.map((provider) => ({
    ...provider,
    name: byKey.get(provider.key)?.name ?? provider.key,
    enabled: byKey.get(provider.key)?.enabled ?? false,
  }));

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title="Shipping"
          description="Zones are the countries you ship to; each offers its options at checkout."
        />
        <ZonesSection zones={zones.items} providers={rateApps} />
        <ProvidersCard providers={rateApps} />
      </VStack>
    </Section>
  );
}
