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
import { getT } from "@/lib/i18n";

import { CategoryForm, DeleteCategory, ImageCard, ProductsCard } from "./category-cards";

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
  const [{ id }, query, t] = await Promise.all([params, searchParams, getT()]);
  const back: Crumb[] = [
    { label: t("common.products"), href: "/products" },
    { label: t("common.categories"), href: "/products/categories" },
  ];
  const cursor = typeof query.cursor === "string" ? query.cursor : undefined;
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Suspense fallback={<DetailSkeleton back={back} />}>
          <Category id={id} cursor={cursor} back={back} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Category({
  id,
  cursor,
  back,
}: {
  id: string;
  cursor: string | undefined;
  back: Crumb[];
}) {
  const client = await sessionClient();
  const [category, tree, products, definitions, fields, me, t] = await Promise.all([
    client.GET("/admin/categories/{id}", { params: { path: { id } } }).then(loaded),
    client.GET("/admin/categories").then(loaded),
    client
      .GET("/admin/categories/{id}/products", { params: { path: { id }, query: { cursor } } })
      .then(loaded),
    // Its custom fields: none shown without `custom_fields:read`.
    definitionsOf("category"),
    client.GET("/admin/categories/{id}/custom-fields", { params: { path: { id } } }),
    requireStaff(),
    getT(),
  ]);
  // Ancestors come nearest first; a breadcrumb reads from the top.
  const trail = category.ancestors.toReversed();

  return (
    <>
      <PageHeader
        title={category.name}
        back={[
          ...back,
          ...trail.map((a) => ({ label: a.name, href: `/products/categories/${a.id}` })),
        ]}
        badges={!category.visible && <Badge label={t("categories.hidden")} />}
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
              <MetadataList title={t("common.identifiers")}>
                <MetadataListItem label={t("categories.category_id")}>
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
