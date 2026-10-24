import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { redirect } from "next/navigation";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import { loadCountries, loaded, sessionClient } from "@/lib/hypha";
import { takenCountries } from "@/lib/regions";

import { CatalogCard } from "./catalog-card";
import { MarketEditor } from "./market-editor";
import { PriceListCard } from "./price-list-card";

const BACK: Crumb = { label: "Markets", href: "/markets" };

/**
 * A market: its details and countries, the catalog of what it sells, and that
 * catalog's price list. Two lists page here, by `products` and `prices` cursors. The
 * way back renders at once; the market streams in behind a skeleton of the page.
 */
export default async function MarketPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const { id } = await params;
  const query = await searchParams;
  const productsCursor = typeof query.products === "string" ? query.products : undefined;
  const pricesCursor = typeof query.prices === "string" ? query.prices : undefined;

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Suspense fallback={<DetailSkeleton back={BACK} />}>
          <Market id={id} productsCursor={productsCursor} pricesCursor={pricesCursor} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Market({
  id,
  productsCursor,
  pricesCursor,
}: {
  id: string;
  productsCursor: string | undefined;
  pricesCursor: string | undefined;
}) {
  const client = await sessionClient();
  // Price lists need `pricing:read`; without it the card says so instead.
  const [market, countries, markets, lists] = await Promise.all([
    client.GET("/admin/markets/{id}", { params: { path: { id } } }).then(loaded),
    loadCountries(),
    client.GET("/admin/markets").then(loaded),
    client.GET("/admin/price-lists"),
  ]);

  const catalogId = market.catalog;
  const [catalog, products] = catalogId
    ? await Promise.all([
        client.GET("/admin/catalogs/{id}", { params: { path: { id: catalogId } } }).then(loaded),
        client.GET("/admin/catalogs/{id}/products", {
          params: { path: { id: catalogId }, query: { cursor: productsCursor } },
        }),
      ])
    : [undefined, undefined];
  // An old or foreign cursor (a hand-edited URL): start the lists again.
  if (products?.response.status === 400) redirect(`/markets/${id}`);

  // The card shows the catalog's first list; the price list pages show them all.
  const listId = catalog?.price_lists[0];
  const [list, prices] = await Promise.all([
    listId
      ? client.GET("/admin/price-lists/{id}", { params: { path: { id: listId } } })
      : undefined,
    listId
      ? client.GET("/admin/price-lists/{id}/fixed-prices", {
          params: { path: { id: listId }, query: { cursor: pricesCursor } },
        })
      : undefined,
  ]);
  if (prices?.response.status === 400) redirect(`/markets/${id}`);

  return (
    <>
      <PageHeader title={market.name} back={BACK} />
      <MarketEditor
        key={[market.name, market.prices_include_tax, ...market.countries].join("|")}
        market={market}
        countries={countries}
        taken={takenCountries(markets.items, market.id)}
        main={
          <>
            <CatalogCard
              market={market}
              catalog={catalog}
              page={products?.data}
              isFirstPage={!productsCursor}
              pricesCursor={pricesCursor}
            />
            {catalog && (
              <PriceListCard
                market={market}
                catalog={catalog}
                canRead={lists.data !== undefined}
                lists={(lists.data?.items ?? []).filter((l) => l.currency === market.currency)}
                list={list?.data}
                page={prices?.data}
                isFirstPage={!pricesCursor}
                productsCursor={productsCursor}
              />
            )}
          </>
        }
      />
    </>
  );
}
