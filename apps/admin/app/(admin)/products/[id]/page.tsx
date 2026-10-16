import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";

import { PageHeader } from "@/components/page-header";
import { ProductStatusBadge } from "@/components/status";
import { loaded, sessionClient } from "@/lib/hypha";

import { ContentForm } from "./content-form";
import { DetailsForm } from "./details-form";
import { MediaSection } from "./media-section";
import { OptionsEditor } from "./options-editor";
import { StatusActions } from "./status-actions";
import { VariantsSection } from "./variants-section";

/**
 * A product, edited in sections that each save on their own (design note 0014, section
 * 8). After a save, each section takes in what the API stored: the forms are keyed by
 * the data they edit (a remount resets them), and the media section, which has uploads
 * running, takes the new list while staying mounted.
 */
export default async function ProductPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const client = await sessionClient();
  const product = loaded(
    await client.GET("/admin/products/{id}", { params: { path: { id } } }),
  );
  const hasOptions = product.options.length > 0;
  const variantTitles = Object.fromEntries(
    product.variants.map((v) => [v.id, v.title ?? (hasOptions ? v.id : "Default")]),
  );

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title={product.title}
          back={{ label: "Products", href: "/products" }}
          badges={<ProductStatusBadge status={product.status} />}
          actions={<StatusActions id={product.id} status={product.status} />}
        />

        <DetailsForm
          key={`${product.title}/${product.handle}`}
          id={product.id}
          title={product.title}
          handle={product.handle}
          isActive={product.status === "active"}
        />

        <ContentForm
          key={JSON.stringify([
            product.description,
            product.vendor,
            product.product_type,
            product.tags,
          ])}
          id={product.id}
          description={product.description ?? ""}
          vendor={product.vendor ?? ""}
          productType={product.product_type ?? ""}
          tags={product.tags}
        />

        <MediaSection
          id={product.id}
          media={product.media}
        />

        <OptionsEditor
          key={JSON.stringify(product.options)}
          id={product.id}
          options={product.options}
          variantTitles={variantTitles}
        />

        <VariantsSection product={product} />

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
