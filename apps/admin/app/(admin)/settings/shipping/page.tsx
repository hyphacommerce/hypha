import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { loaded, sessionClient } from "@/lib/hypha";
import { SETTINGS_CRUMB } from "@/lib/navigation";

import { ProvidersCard, type RateApp } from "./providers";
import { ZonesSection } from "./zones";

/** Settings → Shipping: where the store ships, what each zone offers, and the rate apps
 * that quote live prices. The header renders at once; the zones and providers stream in. */
export default function ShippingSettingsPage() {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Shipping"
          description="Zones are the countries you ship to; each offers its options at checkout."
        />
        <Suspense fallback={<FormSkeleton cards={2} />}>
          <Shipping />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Shipping() {
  const client = await sessionClient();
  const [zones, providers, appsResult] = await Promise.all([
    client.GET("/admin/shipping-zones").then(loaded),
    client.GET("/admin/shipping-providers").then(loaded),
    client.GET("/admin/apps"),
  ]);
  // A rate app's name and switch are its app's. Staff without apps:read still see
  // shipping: the keys, and no switch.
  const apps = appsResult.response.status === 403 ? undefined : loaded(appsResult).items;
  const byKey = new Map(apps?.map((app) => [app.key, app]));
  const rateApps: RateApp[] = providers.items.map((provider) => ({
    ...provider,
    name: byKey.get(provider.key)?.name ?? provider.key,
    enabled: byKey.get(provider.key)?.enabled ?? null,
  }));
  return (
    <>
      <ZonesSection zones={zones.items} providers={rateApps} />
      <ProvidersCard providers={rateApps} />
    </>
  );
}
