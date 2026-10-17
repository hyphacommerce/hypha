import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { loaded, sessionClient } from "@/lib/hypha";

import { CategoryTree } from "./category-tree";

/** Products → Categories (design note 0022): the tree, in order. */
export default async function CategoriesPage() {
  const client = await sessionClient();
  const tree = await client.GET("/admin/categories").then(loaded);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title="Categories"
          back={{ label: "Products", href: "/products" }}
          description="How shoppers browse: a tree of categories. A product can be in any number of them, and a category lists its subcategories' products too."
        />
        <CategoryTree tree={tree.items} />
      </VStack>
    </Section>
  );
}
