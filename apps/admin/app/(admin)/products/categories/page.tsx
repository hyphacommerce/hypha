import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { loaded, sessionClient } from "@/lib/hypha";

import { CategoryTree } from "./category-tree";

/** Products → Categories: the header renders at once; the tree, in order, streams in. */
export default function CategoriesPage() {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title="Categories"
          back={{ label: "Products", href: "/products" }}
          description="How shoppers browse: a tree of categories. A product can be in any number of them, and a category lists its subcategories' products too."
        />
        <Suspense fallback={<CardSkeleton lines={6} />}>
          <Categories />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Categories() {
  const client = await sessionClient();
  const tree = await client.GET("/admin/categories").then(loaded);
  return <CategoryTree tree={tree.items} />;
}
