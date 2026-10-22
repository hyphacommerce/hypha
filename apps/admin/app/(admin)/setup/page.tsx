import { Section } from "@astryxdesign/core/Section";

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
 * there (or on `?step=`, or the first one that isn't done).
 */
export default async function SetupPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const params = await searchParams;
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
  const step = typeof params.step === "string" ? Number(params.step) : undefined;

  return (
    <Section padding={6}>
      <SetupWizard
        data={data}
        initialStep={Number.isInteger(step) ? step : undefined}
        savedStep={store.setup.step}
      />
    </Section>
  );
}
