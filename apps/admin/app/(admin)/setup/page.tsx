import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { FormSkeleton } from "@/components/skeletons";
import {
  loadCountries,
  loadPaymentMethods,
  loaded,
  requireStore,
  sessionClient,
} from "@/lib/hypha";

import { setupDone } from "../setup-checklist";
import { type WizardData, SetupWizard } from "./setup-wizard";

/**
 * The setup wizard: the home page's checklist as one guided flow. Each step is done
 * when the data says so; the store remembers the step the merchant is on and opens
 * there (or on `?step=`, or the first one that isn't done). The whole wizard needs the
 * store's data, so it streams in as one.
 */
export default async function SetupPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const params = await searchParams;
  const step = typeof params.step === "string" ? Number(params.step) : undefined;

  return (
    <Section padding={6}>
      <Suspense fallback={<FormSkeleton cards={2} />}>
        <Wizard initialStep={Number.isInteger(step) ? step : undefined} />
      </Suspense>
    </Section>
  );
}

async function Wizard({ initialStep }: { initialStep: number | undefined }) {
  const client = await sessionClient();
  const [done, store, markets, zones, locations, channels, products, countries, methods] =
    await Promise.all([
      setupDone(),
      requireStore(),
      client.GET("/admin/markets").then(loaded),
      client.GET("/admin/shipping-zones").then(loaded),
      client.GET("/admin/locations").then(loaded),
      client.GET("/admin/channels").then(loaded),
      client
        .GET("/admin/products", { params: { query: { status: ["active"], limit: 50 } } })
        .then(loaded),
      loadCountries(),
      loadPaymentMethods(),
    ]);

  const data: WizardData = {
    done,
    store,
    markets: markets.items,
    zones: zones.items,
    locations: locations.items,
    channels: channels.items,
    paymentMethods: methods ?? [],
    products: products.items,
    countries,
  };

  return <SetupWizard data={data} initialStep={initialStep} savedStep={store.setup.step} />;
}
