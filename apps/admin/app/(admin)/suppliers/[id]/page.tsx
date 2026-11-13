import { Badge } from "@astryxdesign/core/Badge";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { FormSkeleton } from "@/components/skeletons";
import {
  loadCountries,
  loaded,
  locationNames,
  perRequest,
  requireStaff,
  sessionClient,
} from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";
import { type Supplier, daysLabel, termsLabel } from "@/lib/purchasing";

import { NewPurchaseOrder } from "../../purchase-orders/new-purchase-order";
import { PurchaseOrdersTable } from "../../purchase-orders/purchase-orders-table";

import { CatalogueTable } from "./catalogue-table";
import { DeleteSupplier } from "./delete-supplier";
import { SupplierForm } from "./supplier-form";
import { type SupplierTab, SupplierTabs } from "./supplier-tabs";

const BACK: Crumb[] = [{ label: "Suppliers", href: "/suppliers" }];

/** The supplier this request read: the header and the tab share one read. */
const supplierReads = perRequest(async () => new Map<string, Promise<Supplier>>());

async function loadSupplier(id: string): Promise<Supplier> {
  const reads = await supplierReads();
  let read = reads.get(id);
  if (!read) {
    read = sessionClient().then((client) =>
      client.GET("/admin/suppliers/{id}", { params: { path: { id } } }).then(loaded),
    );
    reads.set(id, read);
  }
  return read;
}

function tabFallback(tab: SupplierTab) {
  if (tab === "catalogue") return <CatalogueTable canWrite={false} />;
  if (tab === "orders") return <PurchaseOrdersTable canWrite={false} />;
  return <FormSkeleton cards={4} />;
}

/**
 * A supplier: its details (the form, saved from the save bar) and its catalogue, as tabs
 * (`?tab=catalogue`). The header comes with the supplier; each tab's reads stream in
 * after.
 */
export default async function SupplierPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<SearchParams>;
}) {
  const [{ id }, query] = await Promise.all([params, searchParams]);
  const asked = one(query, "tab");
  const tab: SupplierTab = asked === "catalogue" || asked === "orders" ? asked : "details";
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1080}>
        <Suspense
          fallback={
            <>
              <PageHeader back={BACK} />
              {tabFallback(tab)}
            </>
          }
        >
          <SupplierView id={id} tab={tab} query={query} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function SupplierView({
  id,
  tab,
  query,
}: {
  id: string;
  tab: SupplierTab;
  query: SearchParams;
}) {
  const [supplier, me] = await Promise.all([loadSupplier(id), requireStaff()]);
  const canOrder = hasScope(me.scopes, "purchasing:write") && !supplier.archived;
  return (
    <>
      <PageHeader
        title={supplier.name}
        back={BACK}
        badges={supplier.archived ? <Badge label="Archived" variant="warning" /> : undefined}
        description={`${supplier.currency} · ${termsLabel(supplier.payment_terms)} · lead time ${daysLabel(supplier.lead_time_days)}`}
        actions={canOrder && <NewPurchaseOrder supplier={supplier.id} />}
      />
      <SupplierTabs id={id} tab={tab}>
        <Suspense fallback={tabFallback(tab)}>
          {tab === "catalogue" ? (
            <Catalogue supplier={supplier} params={query} />
          ) : tab === "orders" ? (
            <Orders supplier={supplier} params={query} />
          ) : (
            <Details id={id} />
          )}
        </Suspense>
      </SupplierTabs>
    </>
  );
}

async function Details({ id }: { id: string }) {
  const [supplier, me, countries] = await Promise.all([
    loadSupplier(id),
    requireStaff(),
    loadCountries(),
  ]);
  const canWrite = hasScope(me.scopes, "purchasing:write");
  return (
    <VStack gap={5}>
      <SupplierForm
        key={supplier.updated_at}
        supplier={supplier}
        countries={countries}
        isReadOnly={!canWrite}
      />
      {canWrite && <DeleteSupplier id={supplier.id} name={supplier.name} />}
    </VStack>
  );
}

async function Catalogue({ supplier, params }: { supplier: Supplier; params: SearchParams }) {
  const client = await sessionClient();
  const [me, result] = await Promise.all([
    requireStaff(),
    client.GET("/admin/suppliers/{id}/variants", {
      params: { path: { id: supplier.id }, query: pageQuery(params) },
    }),
  ]);
  return (
    <CatalogueTable
      supplier={supplier.id}
      currency={supplier.currency}
      page={await listPage(result, `/suppliers/${supplier.id}`, params)}
      canWrite={hasScope(me.scopes, "purchasing:write")}
    />
  );
}

async function Orders({ supplier, params }: { supplier: Supplier; params: SearchParams }) {
  const client = await sessionClient();
  const [me, result, locations] = await Promise.all([
    requireStaff(),
    client.GET("/admin/purchase-orders", {
      params: { query: { ...pageQuery(params), supplier: supplier.id } },
    }),
    locationNames(client),
  ]);
  return (
    <PurchaseOrdersTable
      page={await listPage(result, `/suppliers/${supplier.id}`, params)}
      locations={Object.fromEntries(locations)}
      canWrite={hasScope(me.scopes, "purchasing:write")}
    />
  );
}
