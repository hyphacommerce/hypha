import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

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

/** The header and the table's toolbar render at once; the rows stream in. */
export default async function CustomersPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const [params, me] = await Promise.all([searchParams, requireStaff()]);
  const canLabel = hasScope(me.scopes, "customers:write");
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader title="Customers" />
        <Suspense fallback={<CustomersTable canLabel={canLabel} />}>
          <CustomerRows params={params} canLabel={canLabel} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function CustomerRows({ params, canLabel }: { params: SearchParams; canLabel: boolean }) {
  const client = await sessionClient();
  const [definitions, groups] = await Promise.all([
    definitionsOf("customer"),
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
  return (
    <CustomersTable
      page={listPage(result, "/customers", params)}
      fieldFilters={fieldFilters}
      labels={labels}
      groups={groups}
      canLabel={canLabel}
    />
  );
}
