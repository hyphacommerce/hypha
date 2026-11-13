import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, manyOf, one, oneOf } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { pathsById } from "@/lib/categories";
import { fieldQuery, isFilterOffered } from "@/lib/custom-fields";
import { definitionsOf } from "@/lib/custom-fields-server";
import { channelNames, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
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

/** The header and the table's toolbar render at once; the rows stream in. */
export default async function ProductsPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const [params, t] = await Promise.all([searchParams, getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title={t("common.products")}
          actions={
            <>
              <Button
                label={t("common.categories")}
                variant="secondary"
                href="/products/categories"
              />
              <Button label={t("products.add_product")} variant="primary" href="/products/new" />
            </>
          }
        />
        <Suspense fallback={<ProductsTable />}>
          <ProductRows params={params} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function ProductRows({ params }: { params: SearchParams }) {
  const client = await sessionClient();
  // Filterable custom fields: read before the list, which only passes on filters of
  // fields that exist and are filterable.
  const fieldFilters = (await definitionsOf("product")).filter(isFilterOffered);
  const [result, channelName, categories] = await Promise.all([
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
          field: fieldQuery(params, fieldFilters),
        },
      },
    }),
    channelNames(client),
    client.GET("/admin/categories"),
  ]);
  return (
    <ProductsTable
      page={await listPage(result, "/products", params)}
      channels={Object.fromEntries(channelName)}
      categories={Object.fromEntries(pathsById(categories.data?.items ?? []))}
      fieldFilters={fieldFilters}
    />
  );
}
