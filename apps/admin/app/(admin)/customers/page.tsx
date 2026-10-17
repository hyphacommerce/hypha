import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one, oneOf } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { sessionClient } from "@/lib/hypha";
import { CustomersTable } from "@/resources/customers";

const SORTS = ["newest", "oldest", "email_asc", "email_desc", "name_asc", "name_desc"] as const;

export default async function CustomersPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const params = await searchParams;
  const client = await sessionClient();
  const result = await client.GET("/admin/customers", {
    params: {
      query: { ...pageQuery(params), q: one(params, "q"), sort: oneOf(params, "sort", SORTS) },
    },
  });
  const page = listPage(result, "/customers", params);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader title="Customers" />
        <CustomersTable page={page} />
      </VStack>
    </Section>
  );
}
