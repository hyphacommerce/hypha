import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import { PurchaseOrderStatusBadge, ReceiptStatusBadge } from "@/components/status";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";
import { poNumber } from "@/lib/purchasing";

import { PurchaseOrderActions } from "./purchase-order-actions";
import { PurchaseOrderEditor } from "./purchase-order-editor";
import { ReceiptsCard, ReceiveButton } from "./receiving";

const BACK: Crumb[] = [{ label: "Purchase orders", href: "/purchase-orders" }];

/**
 * A purchase order: its lines (outstanding per line, landed cost), extra costs, totals in
 * its currency and the store's, and its details; changed in place while it's a draft or
 * ordered. The header's actions move it along (order, receive, close, cancel, delete); its
 * receipts are listed under it, each to take back.
 */
export default async function PurchaseOrderPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Suspense fallback={<DetailSkeleton back={BACK} />}>
          <PurchaseOrderView id={id} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function PurchaseOrderView({ id }: { id: string }) {
  const client = await sessionClient();
  const [po, me, locations, receipts] = await Promise.all([
    client.GET("/admin/purchase-orders/{id}", { params: { path: { id } } }).then(loaded),
    requireStaff(),
    client.GET("/admin/locations"),
    client.GET("/admin/purchase-orders/{id}/receipts", { params: { path: { id } } }),
  ]);
  const canWrite = hasScope(me.scopes, "purchasing:write");
  const places = (locations.data?.items ?? []).map((l) => ({ id: l.id, name: l.name }));
  return (
    <VStack gap={5}>
      <PageHeader
        title={poNumber(po.number)}
        back={BACK}
        badges={
          <HStack gap={2}>
            <PurchaseOrderStatusBadge status={po.status} />
            {po.status !== "draft" && po.status !== "cancelled" && (
              <ReceiptStatusBadge status={po.receipt_status} />
            )}
          </HStack>
        }
        description={`${po.supplier_name} · ${po.currency}${po.revision > 0 ? ` · revision ${po.revision}` : ""}`}
        actions={
          canWrite && (
            <HStack gap={2} wrap="wrap" vAlign="start">
              <PurchaseOrderActions po={po} />
              <ReceiveButton po={po} places={places} />
            </HStack>
          )
        }
      />
      <PurchaseOrderEditor key={po.updated_at} po={po} locations={places} isReadOnly={!canWrite} />
      <ReceiptsCard
        po={po}
        receipts={receipts.data?.items ?? []}
        places={places}
        canWrite={canWrite}
      />
    </VStack>
  );
}
