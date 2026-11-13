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
import { loadExtensions } from "@/lib/app-extensions";
import { definitionsOf } from "@/lib/custom-fields-server";
import { type Format, getFormat } from "@/lib/get-format";
import { getStoreDefaults } from "@/lib/get-store-t";
import { describe, isMissingScope, loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import type { KeyIn, T } from "@/lib/messages";
import { OPTION_NAME_DEFAULTS } from "@/lib/store-defaults";

import type { Stock } from "../actions";
import { MediaSection } from "./media-section";
import { PricesCard } from "./prices-card";
import { ProductEditor } from "./product-editor";
import { StatusActions } from "./status-actions";

type Product = AdminSchemas["ProductResponse"];

/** Products with more variants read each one's stock when it's opened instead: the
 * page's one read stays small. */
const STOCK_PRELOAD = 500;

/**
 * A product. Details, content, publishing, categories, options and variants are one draft
 * saved with a bar; status, stock and media act at once. The media section, which has
 * uploads running, takes each new list while staying mounted. The way back renders at
 * once; the product streams in behind a skeleton of the page.
 */
export default async function ProductPage({ params }: { params: Promise<{ id: string }> }) {
  const [{ id }, t] = await Promise.all([params, getT()]);
  const back: Crumb = { label: t("common.products"), href: "/products" };
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1320}>
        <Suspense fallback={<DetailSkeleton back={back} />}>
          <ProductDetail id={id} back={back} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function ProductDetail({ id, back }: { id: string; back: Crumb }) {
  const client = await sessionClient();
  const [
    product,
    channels,
    categories,
    locations,
    inventory,
    format,
    extensions,
    productDefinitions,
    variantDefinitions,
    fields,
    me,
    t,
    optionDefaults,
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
    getFormat(),
    // What installed apps add to a product.
    loadExtensions(),
    // Its custom fields and its variants': none shown without
    // `custom_fields:read`.
    definitionsOf("product"),
    definitionsOf("variant"),
    client.GET("/admin/products/{id}/custom-fields", { params: { path: { id } } }),
    requireStaff(),
    getT(),
    // Names offered for a new option are saved with it: in the store's language.
    getStoreDefaults(OPTION_NAME_DEFAULTS),
  ]);
  const stock = locations.data ? stockOf(product, inventory.data?.items ?? []) : undefined;

  return (
    <>
      <PageHeader
        title={product.title}
        back={back}
        badges={<ProductStatusBadge status={product.status} t={t} />}
        actions={<StatusActions id={product.id} title={product.title} status={product.status} />}
      />

      <ProductEditor
        product={product}
        optionNames={OPTION_NAME_DEFAULTS.map((name) => optionDefaults[name])}
        summary={<SummaryCard product={product} t={t} format={format} />}
        channels={channels.data?.items}
        media={<MediaSection id={product.id} media={product.media} />}
        identifiers={
          <Card>
            <MetadataList title={t("common.identifiers")}>
              <MetadataListItem label={t("products.product_id")}>
                <Text type="supporting" color="secondary">
                  {product.id}
                </Text>
              </MetadataListItem>
              <MetadataListItem label={t("common.created")}>
                <Text type="supporting">
                  <DateTime value={product.created_at} />
                </Text>
              </MetadataListItem>
              <MetadataListItem label={t("products.last_changed")}>
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
  const [prices, markets, groups, t] = await Promise.all([
    client.GET("/admin/products/{id}/prices", { params: { path: { id: product.id } } }),
    client.GET("/admin/markets"),
    // Without `customers:read`, none: the card prices for guests only.
    client.GET("/admin/customer-groups"),
    getT(),
  ]);
  if (!prices.data) {
    if (isMissingScope(prices)) return null;
    return (
      <Banner
        status="error"
        title={t("products.prices_unreadable_title")}
        description={prices.error ? await describe(prices.error) : undefined}
      />
    );
  }
  return (
    <PricesCard
      key={product.updated_at}
      productId={product.id}
      initial={prices.data}
      markets={Object.fromEntries((markets.data?.items ?? []).map((m) => [m.id, m.name]))}
      groups={(groups.data?.items ?? []).map((g) => ({ value: g.id, label: g.name }))}
      variantTitles={Object.fromEntries(product.variants.map((v) => [v.id, v.title ?? null]))}
    />
  );
}

/** The variants' stock by variant ID, for products with few enough. */
function stockOf(product: Product, items: Stock[]): Record<string, Stock> {
  if (product.variants.length > STOCK_PRELOAD) return {};
  return Object.fromEntries(items.map((stock) => [stock.variant, stock]));
}

/** What each status means for shoppers. */
const STATUS_HINT: Record<AdminSchemas["ProductStatus"], KeyIn<"products">> = {
  active: "products.summary_status_active",
  draft: "products.summary_status_draft",
  archived: "products.summary_status_archived",
};

/** At a glance: what status means for shoppers, how many variants, what they cost. */
function SummaryCard({ product, t, format }: { product: Product; t: T; format: Format }) {
  const amounts = product.variants.map((v) => v.price.amount);
  const currency = product.variants[0]?.price.currency ?? "EUR";
  const low = Math.min(...amounts);
  const high = Math.max(...amounts);
  const price = (amount: number) => format.money({ amount, currency });

  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          {t("common.summary")}
        </Text>
        <Text type="supporting" color="secondary">
          {t(STATUS_HINT[product.status])}
        </Text>
        <MetadataList>
          <MetadataListItem label={t("common.variants")}>
            <Text>{format.number(product.variants.length)}</Text>
          </MetadataListItem>
          <MetadataListItem label={t("common.price")}>
            <Text>
              {low === high
                ? price(low)
                : t("products.price_range", { min: price(low), max: price(high) })}
            </Text>
          </MetadataListItem>
          <MetadataListItem label={t("products.media")}>
            <Text>{format.number(product.media.length)}</Text>
          </MetadataListItem>
        </MetadataList>
      </VStack>
    </Card>
  );
}
