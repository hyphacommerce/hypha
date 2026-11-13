import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { loaded, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";

import { CategoryTree } from "./category-tree";

/** Products → Categories: the header renders at once; the tree, in order, streams in. */
export default async function CategoriesPage() {
  const t = await getT();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title={t("common.categories")}
          back={{ label: t("common.products"), href: "/products" }}
          description={t("categories.page_description")}
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
