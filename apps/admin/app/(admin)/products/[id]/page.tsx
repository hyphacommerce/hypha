import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { Suspense } from "react";

import { AppSlot } from "@/components/app-extensions/app-slot";
import { DateTime } from "@/components/date-time";
import { type Crumb, PageHeader } from "@/components/page-header";
import { CardSkeleton, DetailSkeleton } from "@/components/skeletons";
import { ProductStatusBadge } from "@/components/status";
import { type Currencies, formatMoney } from "@/lib/format";
import { loadExtensions } from "@/lib/app-extensions";
import { definitionsOf } from "@/lib/custom-fields-server";
import {
  describe,
  isMissingScope,
  loadCurrencies,
  loaded,
  requireStaff,
  sessionClient,
} from "@/lib/hypha";

import type { Stock } from "../actions";
import { MediaSection } from "./media-section";
import { PricesCard } from "./prices-card";
import { ProductEditor } from "./product-editor";
import { StatusActions } from "./status-actions";

type Product = AdminSchemas["ProductResponse"];

/** Products with more variants read each one's stock when it's opened instead: the
 * page's one read stays small. */
const STOCK_PRELOAD = 500;

const BACK: Crumb = { label: "Products", href: "/products" };

/**
 * A product. Details, content, publishing, categories, options and variants are one draft
 * saved with a bar; status, stock and media act at once. The media section, which has
 * uploads running, takes each new list while staying mounted. The way back renders at
 * once; the product streams in behind a skeleton of the page.
 */
export default async function ProductPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1320}>
        <Suspense fallback={<DetailSkeleton back={BACK} />}>
          <ProductDetail id={id} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function ProductDetail({ id }: { id: string }) {
  const client = await sessionClient();
  const [
    product,
    channels,
    categories,
    locations,
    inventory,
    currencies,
    extensions,
    productDefinitions,
    variantDefinitions,
    fields,
    me,
  ] = await Promise.all([
    client.GET("/admin/products/{id}", { params: { path: { id } } }).then(loaded),
    // Without `channels:read` the page shows no Publishing card.
    client.GET("/admin/channels"),
    client.GET("/admin/categories"),
    // Without `inventory:read`, no Stock column.
    client.GET("/admin/locations"),
    // Alongside the product rather than after it: one round trip less, at the price of
    // a read that goes unused for the rare product with more variants than we preload.
    client.GET("/admin/products/{id}/inventory", { params: { path: { id } } }),
    loadCurrencies(),
    // What installed apps add to a product.
    loadExtensions(),
    // Its custom fields and its variants': none shown without
    // `custom_fields:read`.
    definitionsOf("product"),
    definitionsOf("variant"),
    client.GET("/admin/products/{id}/custom-fields", { params: { path: { id } } }),
    requireStaff(),
  ]);
  const stock = locations.data ? stockOf(product, inventory.data?.items ?? []) : undefined;

  return (
    <>
      <PageHeader
        title={product.title}
        back={BACK}
        badges={<ProductStatusBadge status={product.status} />}
        actions={<StatusActions id={product.id} title={product.title} status={product.status} />}
      />

      <ProductEditor
        product={product}
        summary={<SummaryCard product={product} currencies={currencies} />}
        channels={channels.data?.items}
        media={<MediaSection id={product.id} media={product.media} />}
        identifiers={
          <Card>
            <MetadataList title="Identifiers">
              <MetadataListItem label="Product ID">
                <Text type="supporting" color="secondary">
                  {product.id}
                </Text>
              </MetadataListItem>
              <MetadataListItem label="Created">
                <Text type="supporting">
                  <DateTime value={product.created_at} />
                </Text>
              </MetadataListItem>
              <MetadataListItem label="Last changed">
                <Text type="supporting">
                  <DateTime value={product.updated_at} />
                </Text>
              </MetadataListItem>
            </MetadataList>
          </Card>
        }
        categoryTree={categories.data?.items}
        stock={stock}
        locations={locations.data?.items ?? []}
        customFields={
          productDefinitions.length > 0 && fields.data
            ? { definitions: productDefinitions, fields: fields.data, scopes: me.scopes }
            : undefined
        }
        variantFields={
          variantDefinitions.length > 0
            ? { definitions: variantDefinitions, scopes: me.scopes }
            : undefined
        }
      />

      <Suspense fallback={<CardSkeleton lines={5} />}>
        <ProductPrices product={product} />
      </Suspense>

      <AppSlot
        extensions={extensions}
        target="product.details"
        context={{ product_id: product.id }}
      />
    </>
  );
}

/**
 * The Prices card: each variant's price in each market, read after the product so the
 * editor doesn't wait for it. Without `pricing:read` or `markets:read`, no card.
 */
async function ProductPrices({ product }: { product: Product }) {
  const client = await sessionClient();
  const [prices, markets] = await Promise.all([
    client.GET("/admin/products/{id}/prices", { params: { path: { id: product.id } } }),
    client.GET("/admin/markets"),
  ]);
  if (!prices.data) {
    if (isMissingScope(prices)) return null;
    return (
      <Banner
        status="error"
        title="The product's prices couldn't be read"
        description={prices.error ? describe(prices.error) : undefined}
      />
    );
  }
  return (
    <PricesCard
      key={product.updated_at}
      productId={product.id}
      initial={prices.data}
      markets={Object.fromEntries((markets.data?.items ?? []).map((m) => [m.id, m.name]))}
      variantTitles={Object.fromEntries(product.variants.map((v) => [v.id, v.title ?? null]))}
    />
  );
}

/** The variants' stock by variant ID, for products with few enough. */
function stockOf(product: Product, items: Stock[]): Record<string, Stock> {
  if (product.variants.length > STOCK_PRELOAD) return {};
  return Object.fromEntries(items.map((stock) => [stock.variant, stock]));
}

/** At a glance: what status means for shoppers, how many variants, what they cost. */
function SummaryCard({ product, currencies }: { product: Product; currencies: Currencies }) {
  const amounts = product.variants.map((v) => v.price.amount);
  const currency = product.variants[0]?.price.currency ?? "EUR";
  const low = Math.min(...amounts);
  const high = Math.max(...amounts);
  const price = (amount: number) => formatMoney({ amount, currency }, currencies);
  const status = {
    active: "Shoppers can buy it in the channels it's published to.",
    draft: "Only staff see it. Make it active to sell it.",
    archived: "Hidden from shoppers and from the default product list.",
  }[product.status];

  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          Summary
        </Text>
        <Text type="supporting" color="secondary">
          {status}
        </Text>
        <MetadataList>
          <MetadataListItem label="Variants">
            <Text>{product.variants.length}</Text>
          </MetadataListItem>
          <MetadataListItem label="Price">
            <Text>{low === high ? price(low) : `${price(low)} – ${price(high)}`}</Text>
          </MetadataListItem>
          <MetadataListItem label="Media">
            <Text>{product.media.length}</Text>
          </MetadataListItem>
        </MetadataList>
      </VStack>
    </Card>
  );
}
