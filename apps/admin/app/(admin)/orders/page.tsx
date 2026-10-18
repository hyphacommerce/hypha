import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import type { AdminSchemas } from "@hypha/sdk/admin";

import { PageHeader } from "@/components/page-header";
import {
  type SearchParams,
  dateRange,
  manyOf,
  one,
  oneOf,
} from "@/components/resource-table/params";
import { listPage, nextDay, pageQuery, startOfDay } from "@/components/resource-table/server";
import {
  channelNames,
  loadCountries,
  loadCurrencies,
  loadTimeZone,
  sessionClient,
} from "@/lib/hypha";
import { OrdersTable } from "@/resources/orders";

const PAYMENT: AdminSchemas["FinancialStatusDto"][] = [
  "pending",
  "authorized",
  "partially_paid",
  "paid",
  "partially_refunded",
  "refunded",
  "voided",
];
const FULFILMENT: AdminSchemas["FulfillmentStatusDto"][] = [
  "unfulfilled",
  "partially_fulfilled",
  "fulfilled",
];
const SORTS = ["newest", "oldest", "total_desc", "total_asc"] as const;

export default async function OrdersPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const params = await searchParams;
  const client = await sessionClient();
  const placed = dateRange(params, "placed");
  const cancelled = one(params, "cancelled");
  // Days of the store's calendar: the placed range starts and ends at its midnights, so
  // only a placed filter waits for the store's time zone (read once per request, shared
  // with the layout).
  const timeZone = placed ? await loadTimeZone() : "UTC";
  const [result, markets, channelName, currencies, countries] = await Promise.all([
    client.GET("/admin/orders", {
      params: {
        query: {
          ...pageQuery(params),
          q: one(params, "q"),
          financial_status: manyOf(params, "financial_status", PAYMENT),
          fulfillment_status: manyOf(params, "fulfillment_status", FULFILMENT),
          cancelled: cancelled === "true" ? true : cancelled === "false" ? false : undefined,
          channel: one(params, "channel"),
          country: one(params, "country"),
          currency: one(params, "currency"),
          placed_from: placed && startOfDay(placed.start, timeZone),
          placed_before: placed && startOfDay(nextDay(placed.end), timeZone),
          sort: oneOf(params, "sort", SORTS),
        },
      },
    }),
    client.GET("/admin/markets"),
    channelNames(client),
    loadCurrencies(),
    loadCountries(),
  ]);
  const page = listPage(result, "/orders", params);

  const sold = markets.data?.items ?? [];
  const countryName = new Map(countries.map((c) => [c.code, c.name]));
  const soldTo = [...new Set(sold.flatMap((m) => m.countries))]
    .map((code) => ({ value: code, label: countryName.get(code) ?? code }))
    .toSorted((a, b) => a.label.localeCompare(b.label));

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader title="Orders" />
        <OrdersTable
          page={page}
          currencies={currencies}
          channels={Object.fromEntries(channelName)}
          countries={soldTo}
          orderCurrencies={[...new Set(sold.map((m) => m.currency))].toSorted()}
        />
      </VStack>
    </Section>
  );
}
