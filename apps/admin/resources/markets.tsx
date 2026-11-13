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
import { useT } from "@/lib/use-t";

/** A market with its countries already named ("Croatia, Slovenia"). */
export type MarketRow = AdminSchemas["MarketResponse"] & { countryNames: string };

export function MarketsTable({ markets }: { markets: MarketRow[] }) {
  const t = useT<"markets">();
  const columns: TableColumn<MarketRow>[] = [
    {
      key: "name",
      header: t("common.market"),
      renderCell: (m) => (
        <Link href={`/markets/${m.id}`} weight="semibold" color="primary">
          {m.name}
        </Link>
      ),
    },
    { key: "countryNames", header: t("common.countries"), width: proportional(3) },
    { key: "currency", header: t("common.currency"), width: pixel(100) },
    {
      key: "prices",
      header: t("markets.list_column_prices"),
      width: pixel(130),
      renderCell: (m) => (
        <Text color="secondary">
          {m.prices_include_tax
            ? t("markets.list_prices_include_tax")
            : t("markets.list_prices_exclude_tax")}
        </Text>
      ),
    },
    {
      key: "catalog",
      header: t("markets.list_column_catalog"),
      width: pixel(120),
      renderCell: (m) =>
        m.catalog ? (
          <Badge label={t("markets.list_catalog_ready")} variant="success" />
        ) : (
          <Badge label={t("markets.list_catalog_none")} variant="warning" />
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
      searchLabel={t("markets.list_search_label")}
      compare={{
        name: byText((m: MarketRow) => m.name),
        currency: byText((m: MarketRow) => m.currency),
      }}
      empty={{
        icon: <GlobeEuropeAfricaIcon width={32} />,
        title: t("markets.list_empty_title"),
        description: t("markets.list_empty_description"),
        action: <Button label={t("markets.add_market")} href="/markets/new" />,
      }}
    />
  );
}
