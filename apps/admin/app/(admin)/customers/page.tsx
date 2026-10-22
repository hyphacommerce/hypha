import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one, oneOf } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { fieldQuery, isFilterOffered } from "@/lib/custom-fields";
import { definitionsOf } from "@/lib/custom-fields-server";
import { requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";
import { wellKnownField } from "@/lib/well-known-fields";
import { CustomersTable } from "@/resources/customers";

const SORTS = ["newest", "oldest", "email_asc", "email_desc", "name_asc", "name_desc"] as const;

export default async function CustomersPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const params = await searchParams;
  const client = await sessionClient();
  const [definitions, me, groups] = await Promise.all([
    definitionsOf("customer"),
    requireStaff(),
    client.GET("/admin/customer-groups").then((r) => r.data?.items ?? []),
  ]);
  const group = one(params, "group");
  // Filterable custom fields; the labels field also gets a column and the selection
  // bar's actions.
  const fieldFilters = definitions.filter(isFilterOffered);
  const labels = wellKnownField(definitions, "labels");
  const result = await client.GET("/admin/customers", {
    params: {
      query: {
        ...pageQuery(params),
        q: one(params, "q"),
        sort: oneOf(params, "sort", SORTS),
        field: fieldQuery(params, fieldFilters),
        custom_fields: labels?.field,
        group: groups.some((g) => g.id === group) ? group : undefined,
      },
    },
  });
  const page = listPage(result, "/customers", params);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader title="Customers" />
        <CustomersTable
          page={page}
          fieldFilters={fieldFilters}
          labels={labels}
          groups={groups}
          canLabel={hasScope(me.scopes, "customers:write")}
        />
      </VStack>
    </Section>
  );
}
