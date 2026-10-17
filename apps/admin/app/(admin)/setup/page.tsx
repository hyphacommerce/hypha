import { Section } from "@astryxdesign/core/Section";

import { loadCountries, loaded, sessionClient } from "@/lib/hypha";

import { setupSteps } from "../setup-checklist";
import { type WizardData, SetupWizard } from "./setup-wizard";

/**
 * The setup wizard (design note 0024, AU11): the home page's checklist as one guided
 * flow. Nothing about it is stored; each step is done when the data says so, and opens
 * on the first one that isn't (or `?step=`).
 */
export default async function SetupPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const params = await searchParams;
  const client = await sessionClient();
  const [steps, store, markets, zones, locations, channels, products, countries] =
    await Promise.all([
      setupSteps(),
      client.GET("/admin/store").then(loaded),
      client.GET("/admin/markets").then(loaded),
      client.GET("/admin/shipping-zones").then(loaded),
      client.GET("/admin/locations").then(loaded),
      client.GET("/admin/channels").then(loaded),
      client.GET("/admin/products", { params: { query: { status: ["active"], limit: 50 } } }).then(loaded),
      loadCountries(),
    ]);

  const data: WizardData = {
    done: Object.fromEntries(steps.map((s) => [s.key, s.isDone])) as WizardData["done"],
    store,
    markets: markets.items,
    zones: zones.items,
    locations: locations.items,
    channels: channels.items,
    products: products.items,
    countries,
  };
  const step = typeof params.step === "string" ? Number(params.step) : undefined;

  return (
    <Section padding={6}>
      <SetupWizard data={data} initialStep={Number.isInteger(step) ? step : undefined} />
    </Section>
  );
}
