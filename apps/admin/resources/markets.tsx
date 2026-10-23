"use client";

// The markets table: a whole list, sorted and searched here.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { GlobeEuropeAfricaIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { ClientResourceTable, byText } from "@/components/resource-table/resource-table";

/** A market with its countries already named ("Croatia, Slovenia"). */
export type MarketRow = AdminSchemas["MarketResponse"] & { countryNames: string };

export function MarketsTable({ markets }: { markets: MarketRow[] }) {
  const columns: TableColumn<MarketRow>[] = [
    {
      key: "name",
      header: "Market",
      renderCell: (m) => (
        <Link href={`/markets/${m.id}`} weight="semibold" color="primary">
          {m.name}
        </Link>
      ),
    },
    { key: "countryNames", header: "Countries", width: proportional(3) },
    { key: "currency", header: "Currency", width: pixel(100) },
    {
      key: "prices",
      header: "Prices",
      width: pixel(130),
      renderCell: (m) => (
        <Text color="secondary">{m.prices_include_tax ? "Include tax" : "Exclude tax"}</Text>
      ),
    },
    {
      key: "catalog",
      header: "Catalog",
      width: pixel(120),
      renderCell: (m) =>
        m.catalog ? (
          <Badge label="Ready" variant="success" />
        ) : (
          <Badge label="None yet" variant="warning" />
        ),
    },
  ];
  return (
    <ClientResourceTable<MarketRow>
      rows={markets}
      columns={columns}
      idKey="id"
      rowHref={(m) => `/markets/${m.id}`}
      searchText={(m) => `${m.name} ${m.countryNames} ${m.currency} ${m.countries.join(" ")}`}
      searchLabel="Search markets, countries or currencies"
      compare={{
        name: byText((m: MarketRow) => m.name),
        currency: byText((m: MarketRow) => m.currency),
      }}
      empty={{
        icon: <GlobeEuropeAfricaIcon width={32} />,
        title: "No markets yet",
        description: "A storefront sells only to countries in a market. Add one to start selling.",
        action: <Button label="Add market" href="/markets/new" />,
      }}
    />
  );
}
