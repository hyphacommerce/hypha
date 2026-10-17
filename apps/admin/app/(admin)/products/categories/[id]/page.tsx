import { Card } from "@astryxdesign/core/Card";
import { Breadcrumbs, BreadcrumbItem } from "@astryxdesign/core/Breadcrumbs";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { Badge } from "@astryxdesign/core/Badge";

import { loaded, sessionClient } from "@/lib/hypha";

import { CategoryForm, DeleteCategory, ImageCard, ProductsCard } from "./category-cards";

/** A category: its details, image and products (design note 0022, section 5). */
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
  const client = await sessionClient();
  const [category, tree, products] = await Promise.all([
    client.GET("/admin/categories/{id}", { params: { path: { id } } }).then(loaded),
    client.GET("/admin/categories").then(loaded),
    client
      .GET("/admin/categories/{id}/products", { params: { path: { id }, query: { cursor } } })
      .then(loaded),
  ]);
  // Ancestors come nearest first; a breadcrumb reads from the top.
  const trail = [...category.ancestors].reverse();

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <VStack gap={2}>
          <Breadcrumbs variant="supporting">
            <BreadcrumbItem href="/products">Products</BreadcrumbItem>
            <BreadcrumbItem href="/products/categories">Categories</BreadcrumbItem>
            {trail.map((a) => (
              <BreadcrumbItem key={a.id} href={`/products/categories/${a.id}`}>
                {a.name}
              </BreadcrumbItem>
            ))}
            <BreadcrumbItem>{category.name}</BreadcrumbItem>
          </Breadcrumbs>
          <HStack gap={4} vAlign="center" hAlign="between" wrap="wrap">
            <HStack gap={3} vAlign="center">
              <Text type="display-1" as="h1">
                {category.name}
              </Text>
              {!category.visible && <Badge label="Hidden" />}
            </HStack>
            <DeleteCategory id={category.id} name={category.name} />
          </HStack>
        </VStack>

        <CategoryForm key={category.updated_at} category={category} tree={tree.items} />
        <ImageCard id={category.id} image={category.image ?? null} />
        <ProductsCard id={category.id} page={products} isFirstPage={!cursor} />

        <Card>
          <MetadataList title="Identifiers">
            <MetadataListItem label="Category ID">
              <Text type="supporting" color="secondary">
                {category.id}
              </Text>
            </MetadataListItem>
          </MetadataList>
        </Card>
      </VStack>
    </Section>
  );
}
