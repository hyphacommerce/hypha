import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { locationNames, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { hasScope } from "@/lib/navigation";

import { NewPurchaseOrder } from "./new-purchase-order";
import { PurchaseOrdersTable } from "./purchase-orders-table";

const STATUSES = ["draft", "ordered", "closed", "cancelled"] as const;
const RECEIPTS = ["none", "partial", "received"] as const;
const SORTS = ["number_desc", "number_asc", "expected_at"] as const;

function pick<T extends string>(value: string | undefined, allowed: readonly T[]): T | undefined {
  return allowed.find((a) => a === value);
}

/**
 * What the store has ordered from its suppliers: each order's supplier, where it goes,
 * how far it got and its total. The header and the table's toolbar render at once; the
 * rows stream in.
 */
export default async function PurchaseOrdersPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const [params, me, t] = await Promise.all([searchParams, requireStaff(), getT()]);
  const canWrite = hasScope(me.scopes, "purchasing:write");
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title={t("purchasing.list_title")}
          description={t("purchasing.list_description")}
          actions={canWrite && <NewPurchaseOrder />}
        />
        <Suspense fallback={<PurchaseOrdersTable canWrite={canWrite} />}>
          <Rows params={params} canWrite={canWrite} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Rows({ params, canWrite }: { params: SearchParams; canWrite: boolean }) {
  const client = await sessionClient();
  const [result, locations] = await Promise.all([
    client.GET("/admin/purchase-orders", {
      params: {
        query: {
          ...pageQuery(params),
          status: pick(one(params, "status"), STATUSES),
          receipt_status: pick(one(params, "receipt_status"), RECEIPTS),
          supplier: one(params, "supplier"),
          sort: pick(one(params, "sort"), SORTS),
        },
      },
    }),
    locationNames(client),
  ]);
  return (
    <PurchaseOrdersTable
      page={await listPage(result, "/purchase-orders", params)}
      locations={Object.fromEntries(locations)}
      canWrite={canWrite}
    />
  );
}
