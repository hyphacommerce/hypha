import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { Badge } from "@astryxdesign/core/Badge";
import { Suspense } from "react";

import { CustomFieldsCard } from "@/components/custom-fields/custom-fields-card";
import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import { definitionsOf } from "@/lib/custom-fields-server";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";

import { CategoryForm, DeleteCategory, ImageCard, ProductsCard } from "./category-cards";

const BACK: Crumb[] = [
  { label: "Products", href: "/products" },
  { label: "Categories", href: "/products/categories" },
];

/**
 * A category: its details, image and products. The way back renders at once; the
 * category (and with it the ancestors in its trail) streams in behind a skeleton of
 * the page.
 */
export default async function CategoryPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const { id } = await params;
  const query = await searchParams;
  const cursor = typeof query.cursor === "string" ? query.cursor : undefined;
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Suspense fallback={<DetailSkeleton back={BACK} />}>
          <Category id={id} cursor={cursor} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Category({ id, cursor }: { id: string; cursor: string | undefined }) {
  const client = await sessionClient();
  const [category, tree, products, definitions, fields, me] = await Promise.all([
    client.GET("/admin/categories/{id}", { params: { path: { id } } }).then(loaded),
    client.GET("/admin/categories").then(loaded),
    client
      .GET("/admin/categories/{id}/products", { params: { path: { id }, query: { cursor } } })
      .then(loaded),
    // Its custom fields: none shown without `custom_fields:read`.
    definitionsOf("category"),
    client.GET("/admin/categories/{id}/custom-fields", { params: { path: { id } } }),
    requireStaff(),
  ]);
  // Ancestors come nearest first; a breadcrumb reads from the top.
  const trail = category.ancestors.toReversed();

  return (
    <>
      <PageHeader
        title={category.name}
        back={[
          ...BACK,
          ...trail.map((a) => ({ label: a.name, href: `/products/categories/${a.id}` })),
        ]}
        badges={!category.visible && <Badge label="Hidden" />}
        actions={<DeleteCategory id={category.id} name={category.name} />}
      />

      <CategoryForm
        key={category.updated_at}
        category={category}
        tree={tree.items}
        main={<ProductsCard id={category.id} page={products} isFirstPage={!cursor} />}
        side={
          <>
            <ImageCard id={category.id} image={category.image ?? null} />
            {definitions.length > 0 && fields.data && (
              <CustomFieldsCard
                ownerType="category"
                ownerId={category.id}
                definitions={definitions}
                fields={fields.data}
                scopes={me.scopes}
              />
            )}
            <Card>
              <MetadataList title="Identifiers">
                <MetadataListItem label="Category ID">
                  <Text type="supporting" color="secondary">
                    {category.id}
                  </Text>
                </MetadataListItem>
              </MetadataList>
            </Card>
          </>
        }
      />
    </>
  );
}
