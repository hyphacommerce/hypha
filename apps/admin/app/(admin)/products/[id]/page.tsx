import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";

import { PageHeader } from "@/components/page-header";
import { ProductStatusBadge } from "@/components/status";
import { loaded, sessionClient } from "@/lib/hypha";

import { MediaSection } from "./media-section";
import { ProductEditor } from "./product-editor";
import { PublishingCard } from "./publishing-card";
import { StatusActions } from "./status-actions";

/**
 * A product. Details, content, options and variants are one draft saved with a bar
 * (design note 0021, section 8); status, publishing and media act at once. The media
 * section, which has uploads running, takes each new list while staying mounted.
 */
export default async function ProductPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const client = await sessionClient();
  const [product, channels] = await Promise.all([
    client.GET("/admin/products/{id}", { params: { path: { id } } }).then(loaded),
    // Without `channels:read` the page shows no Publishing card.
    client.GET("/admin/channels"),
  ]);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title={product.title}
          back={{ label: "Products", href: "/products" }}
          badges={<ProductStatusBadge status={product.status} />}
          actions={<StatusActions id={product.id} status={product.status} />}
        />

        <ProductEditor
          product={product}
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
        />

        <Card>
          <MetadataList title="Identifiers">
            <MetadataListItem label="Product ID">
              <Text type="supporting" color="secondary">
                {product.id}
              </Text>
            </MetadataListItem>
          </MetadataList>
        </Card>
      </VStack>
    </Section>
  );
}
