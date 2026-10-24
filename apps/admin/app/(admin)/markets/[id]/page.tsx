import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { redirect } from "next/navigation";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import { loadCountries, loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";
import { takenCountries } from "@/lib/regions";

import { CatalogCard } from "./catalog-card";
import { MarketEditor } from "./market-editor";
import { PriceListsCard } from "./price-lists-card";

const BACK: Crumb = { label: "Markets", href: "/markets" };

/**
 * A market: its details and countries, the catalog of what it sells (paged by the
 * `products` cursor), and the catalog's price lists. The way back renders at once; the
 * market streams in behind a skeleton of the page.
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

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Suspense fallback={<DetailSkeleton back={BACK} />}>
          <Market id={id} productsCursor={productsCursor} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Market({ id, productsCursor }: { id: string; productsCursor: string | undefined }) {
  const client = await sessionClient();
  // Price lists need `pricing:read`; without it the card says so instead. The catalog's
  // own (at most 50) come in one page; the first 100 of the rest are offered to add.
  const [market, countries, markets, own, all, me] = await Promise.all([
    client.GET("/admin/markets/{id}", { params: { path: { id } } }).then(loaded),
    loadCountries(),
    client.GET("/admin/markets").then(loaded),
    client.GET("/admin/price-lists", { params: { query: { market: id, limit: 100 } } }),
    client.GET("/admin/price-lists", { params: { query: { sort: "name_asc", limit: 100 } } }),
    requireStaff(),
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
            />
            {catalog && (
              <PriceListsCard
                market={market}
                catalog={catalog}
                canRead={own.data !== undefined}
                canWrite={hasScope(me.scopes, "markets:write")}
                canCreate={hasScope(me.scopes, "pricing:write")}
                lists={catalog.price_lists.flatMap((listId) => {
                  const found = own.data?.items.find((l) => l.id === listId);
                  return found ? [found] : [];
                })}
                offered={(all.data?.items ?? []).filter(
                  (l) => l.currency === market.currency && !catalog.price_lists.includes(l.id),
                )}
              />
            )}
          </>
        }
      />
    </>
  );
}
