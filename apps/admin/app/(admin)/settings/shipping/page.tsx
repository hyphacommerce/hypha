import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { loaded, sessionClient } from "@/lib/hypha";

import { ProvidersCard } from "./providers";
import { ZonesSection } from "./zones";

/** Settings → Shipping (design note 0015, section 8): where the store ships, what each
 * zone offers, and the rate apps that quote live prices. */
export default async function ShippingSettingsPage() {
  const client = await sessionClient();
  const [zones, providers] = await Promise.all([
    client.GET("/admin/shipping-zones").then(loaded),
    client.GET("/admin/shipping-providers").then(loaded),
  ]);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title="Shipping"
          description="Zones are the countries you ship to; each offers its options at checkout."
        />
        <ZonesSection zones={zones.items} providers={providers.items} />
        <ProvidersCard providers={providers.items} />
      </VStack>
    </Section>
  );
}
