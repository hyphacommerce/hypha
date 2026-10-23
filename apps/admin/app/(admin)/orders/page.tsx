import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { Suspense } from "react";

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
import { fieldQuery, isFilterOffered } from "@/lib/custom-fields";
import { definitionsOf } from "@/lib/custom-fields-server";
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

/** The header and the table's toolbar render at once; the rows stream in. */
export default async function OrdersPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const params = await searchParams;
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader title="Orders" />
        <Suspense fallback={<OrdersTable />}>
          <OrderRows params={params} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function OrderRows({ params }: { params: SearchParams }) {
  const client = await sessionClient();
  const placed = dateRange(params, "placed");
  const cancelled = one(params, "cancelled");
  // Days of the store's calendar: the placed range starts and ends at its midnights, so
  // only a placed filter waits for the store's time zone (read once per request, shared
  // with the layout).
  const timeZone = placed ? await loadTimeZone() : "UTC";
  // Filterable custom fields.
  const fieldFilters = (await definitionsOf("order")).filter(isFilterOffered);
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
          // A customer page's "View all"; no chip offers it.
          customer: one(params, "customer"),
          country: one(params, "country"),
          currency: one(params, "currency"),
          placed_from: placed && startOfDay(placed.start, timeZone),
          placed_before: placed && startOfDay(nextDay(placed.end), timeZone),
          sort: oneOf(params, "sort", SORTS),
          field: fieldQuery(params, fieldFilters),
        },
      },
    }),
    client.GET("/admin/markets"),
    channelNames(client),
    loadCurrencies(),
    loadCountries(),
  ]);

  const sold = markets.data?.items ?? [];
  const countryName = new Map(countries.map((c) => [c.code, c.name]));
  const soldTo = [...new Set(sold.flatMap((m) => m.countries))]
    .map((code) => ({ value: code, label: countryName.get(code) ?? code }))
    .toSorted((a, b) => a.label.localeCompare(b.label));

  return (
    <OrdersTable
      page={listPage(result, "/orders", params)}
      currencies={currencies}
      channels={Object.fromEntries(channelName)}
      countries={soldTo}
      orderCurrencies={[...new Set(sold.map((m) => m.currency))].toSorted()}
      fieldFilters={fieldFilters}
    />
  );
}
