import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";

import { DateTime } from "@/components/date-time";
import { PageHeader } from "@/components/page-header";
import { ProductStatusBadge } from "@/components/status";
import { type Currencies, formatMoney } from "@/lib/format";
import { loadCurrencies, loaded, sessionClient } from "@/lib/hypha";

import type { Stock } from "../actions";
import { MediaSection } from "./media-section";
import { ProductEditor } from "./product-editor";
import { PublishingCard } from "./publishing-card";
import { StatusActions } from "./status-actions";

type Product = AdminSchemas["ProductResponse"];

/** Products with more variants read each one's stock when it's opened instead. */
const STOCK_PRELOAD = 100;

/**
 * A product. Details, content, options and variants are one draft saved with a bar
 * (design note 0021, section 8); status, publishing, stock and media act at once. The
 * media section, which has uploads running, takes each new list while staying mounted.
 */
export default async function ProductPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const client = await sessionClient();
  const [product, channels, categories, locations, currencies] = await Promise.all([
    client.GET("/admin/products/{id}", { params: { path: { id } } }).then(loaded),
    // Without `channels:read` the page shows no Publishing card.
    client.GET("/admin/channels"),
    client.GET("/admin/categories"),
    // Without `inventory:read`, no Stock column.
    client.GET("/admin/locations"),
    loadCurrencies(),
  ]);
  const stock = locations.data ? await loadStock(client, product) : undefined;

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1320}>
        <PageHeader
          title={product.title}
          back={{ label: "Products", href: "/products" }}
          badges={<ProductStatusBadge status={product.status} />}
          actions={<StatusActions id={product.id} status={product.status} />}
        />

        <ProductEditor
          product={product}
          summary={<SummaryCard product={product} currencies={currencies} />}
          publishing={
            channels.data && (
              <PublishingCard
                key={product.channels.join()}
                id={product.id}
                published={product.channels}
                channels={channels.data.items}
                isActive={product.status === "active"}
              />
            )
          }
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
                  <Text type="supporting"><DateTime value={product.created_at} /></Text>
                </MetadataListItem>
                <MetadataListItem label="Last changed">
                  <Text type="supporting"><DateTime value={product.updated_at} /></Text>
                </MetadataListItem>
              </MetadataList>
            </Card>
          }
          categoryTree={categories.data?.items}
          stock={stock}
          locations={locations.data?.items ?? []}
        />
      </VStack>
    </Section>
  );
}

/** The variants' stock by variant ID, read together for products with few enough. */
async function loadStock(
  client: Awaited<ReturnType<typeof sessionClient>>,
  product: Product,
): Promise<Record<string, Stock>> {
  if (product.variants.length > STOCK_PRELOAD) return {};
  const read = await Promise.all(
    product.variants.map((v) =>
      client.GET("/admin/variants/{id}/inventory", { params: { path: { id: v.id } } }),
    ),
  );
  return Object.fromEntries(
    read.flatMap((result, i) => (result.data ? [[product.variants[i].id, result.data]] : [])),
  );
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
