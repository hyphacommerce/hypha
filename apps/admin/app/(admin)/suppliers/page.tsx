import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { loadStore, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";

import { NewSupplier } from "./new-supplier";
import { SuppliersTable } from "./suppliers-table";

/**
 * Who the store buys stock from: each supplier's main contact, currency, payment terms
 * and lead time. The header and the table's toolbar render at once; the rows stream in.
 */
export default async function SuppliersPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const [params, me, store] = await Promise.all([searchParams, requireStaff(), loadStore()]);
  const canWrite = hasScope(me.scopes, "purchasing:write");
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title="Suppliers"
          description="Who you buy stock from, on what terms, and what each sells you at what price."
          actions={canWrite && <NewSupplier currency={store?.currency ?? null} />}
        />
        <Suspense fallback={<SuppliersTable canWrite={canWrite} />}>
          <SupplierRows params={params} canWrite={canWrite} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function SupplierRows({ params, canWrite }: { params: SearchParams; canWrite: boolean }) {
  const client = await sessionClient();
  const archived = one(params, "archived");
  const result = await client.GET("/admin/suppliers", {
    params: {
      query: {
        ...pageQuery(params),
        q: one(params, "q")?.trim() || undefined,
        archived: archived === "true" ? true : archived === "false" ? false : undefined,
      },
    },
  });
  return <SuppliersTable page={await listPage(result, "/suppliers", params)} canWrite={canWrite} />;
}
