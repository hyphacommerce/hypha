import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import type { AdminSchemas } from "@hypha/sdk/admin";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, manyOf, one, oneOf } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { pathsById } from "@/lib/categories";
import { channelNames, loadCurrencies, sessionClient } from "@/lib/hypha";
import { ProductsTable } from "@/resources/products";

const STATUSES: AdminSchemas["ProductStatus"][] = ["active", "draft", "archived"];
const SORTS = [
  "newest",
  "oldest",
  "title_asc",
  "title_desc",
  "updated_desc",
  "updated_asc",
] as const;

export default async function ProductsPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const params = await searchParams;
  const client = await sessionClient();
  const [result, channelName, currencies, categories] = await Promise.all([
    client.GET("/admin/products", {
      params: {
        query: {
          ...pageQuery(params),
          q: one(params, "q"),
          status: manyOf(params, "status", STATUSES),
          vendor: one(params, "vendor"),
          product_type: one(params, "product_type"),
          tag: one(params, "tag"),
          category: one(params, "category"),
          channel: one(params, "channel"),
          sort: oneOf(params, "sort", SORTS),
        },
      },
    }),
    channelNames(client),
    loadCurrencies(),
    client.GET("/admin/categories"),
  ]);
  const page = listPage(result, "/products", params);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title="Products"
          actions={
            <>
              <Button label="Categories" variant="secondary" href="/products/categories" />
              <Button label="Add product" variant="primary" href="/products/new" />
            </>
          }
        />
        <ProductsTable
          page={page}
          currencies={currencies}
          channels={Object.fromEntries(channelName)}
          categories={Object.fromEntries(pathsById(categories.data?.items ?? []))}
        />
      </VStack>
    </Section>
  );
}
