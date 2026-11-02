import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { Suspense } from "react";

import { DetailSkeleton } from "@/components/skeletons";
import { formatMoney, formatUnitCost } from "@/lib/format";
import { loadCurrencies, loadStore, loaded, sessionClient } from "@/lib/hypha";
import { poNumber, termsLabel } from "@/lib/purchasing";

import { PrintButton } from "./print-button";

const KINDS: Record<string, string> = {
  shipping: "Shipping",
  duties: "Duties",
  fees: "Fees",
  other: "Other",
};

/** A day as the order means it: dates are chosen as days, at noon UTC. */
function day(rfc3339: string | null | undefined): string | null {
  if (!rfc3339) return null;
  return new Intl.DateTimeFormat("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
    timeZone: "UTC",
  }).format(new Date(rfc3339));
}

/**
 * A purchase order as a document to print or save as a PDF: what the supplier gets by
 * email, in the order's currency, with where the goods go.
 */
export default async function PrintPurchaseOrderPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  return (
    <Section padding={6}>
      <Suspense
        fallback={
          <DetailSkeleton back={[{ label: "Purchase order", href: `/purchase-orders/${id}` }]} />
        }
      >
        <PrintView id={id} />
      </Suspense>
    </Section>
  );
}

async function PrintView({ id }: { id: string }) {
  const client = await sessionClient();
  const [po, store, currencies] = await Promise.all([
    client.GET("/admin/purchase-orders/{id}", { params: { path: { id } } }).then(loaded),
    loadStore(),
    loadCurrencies(),
  ]);
  const [supplier, location] = await Promise.all([
    client.GET("/admin/suppliers/{id}", { params: { path: { id: po.supplier } } }),
    po.destination
      ? client.GET("/admin/locations/{id}", { params: { path: { id: po.destination } } })
      : undefined,
  ]);
  const address = location?.data?.address;
  const costs = po.costs.filter((c) => c.amount.currency === po.currency);
  return (
    <VStack gap={5} maxWidth={860} className="po-print">
      <HStack gap={2} className="no-print">
        <Button label="Back" variant="ghost" href={`/purchase-orders/${po.id}`} />
        <PrintButton />
      </HStack>
      <HStack hAlign="between" vAlign="start" gap={4} wrap="wrap">
        <VStack gap={1}>
          <Text type="display-2" as="h1">
            Purchase order {poNumber(po.number)}
          </Text>
          {po.revision > 0 && <Text color="secondary">Revision {po.revision}</Text>}
          <Text>{store?.name}</Text>
          {store?.contact_email && <Text color="secondary">{store.contact_email}</Text>}
        </VStack>
        <VStack gap={1}>
          {po.ordered_at && <Text>Ordered {day(po.ordered_at)}</Text>}
          {po.expected_at && <Text>Expected by {day(po.expected_at)}</Text>}
          {po.supplier_reference && <Text>Your reference {po.supplier_reference}</Text>}
          {po.payment_terms && (
            <Text>
              Payment terms: {termsLabel(po.payment_terms)}
              {po.payment_terms.note ? ` (${po.payment_terms.note})` : ""}
            </Text>
          )}
        </VStack>
      </HStack>
      <HStack gap={8} vAlign="start" wrap="wrap">
        <VStack gap={1}>
          <Text weight="semibold">Supplier</Text>
          <Text>{po.supplier_name ?? supplier.data?.name}</Text>
          {supplier.data?.tax_number && <Text color="secondary">{supplier.data.tax_number}</Text>}
        </VStack>
        {location?.data && (
          <VStack gap={1}>
            <Text weight="semibold">Ship to</Text>
            <Text>{location.data.name}</Text>
            {address && (
              <>
                {address.company && <Text>{address.company}</Text>}
                <Text>{address.line1}</Text>
                {address.line2 && <Text>{address.line2}</Text>}
                <Text>
                  {address.postal_code ? `${address.postal_code} ` : ""}
                  {address.city}
                </Text>
                {address.province && <Text>{address.province_name ?? address.province}</Text>}
                <Text>{address.country}</Text>
                {address.phone && <Text>{address.phone}</Text>}
              </>
            )}
          </VStack>
        )}
      </HStack>
      <table>
        <thead>
          <tr>
            <th>Item</th>
            <th>Your code</th>
            <th>Our SKU</th>
            <th className="amount">Quantity</th>
            <th className="amount">Unit cost</th>
            <th className="amount">Total</th>
          </tr>
        </thead>
        <tbody>
          {po.lines.map((line) => (
            <tr key={line.variant}>
              <td>
                {line.product_title}
                {line.variant_title ? ` · ${line.variant_title}` : ""}
              </td>
              <td>{line.supplier_sku ?? ""}</td>
              <td>{line.sku ?? ""}</td>
              <td className="amount">{line.quantity - line.cancelled}</td>
              <td className="amount">{formatUnitCost(line.unit_cost, currencies)}</td>
              <td className="amount">{formatMoney(line.total, currencies)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <VStack gap={1} hAlign="end">
        <Text>Subtotal {formatMoney(po.subtotal, currencies)}</Text>
        {costs.map((cost, i) => (
          // Cost rows have no ID; their order is the order's.
          // oxlint-disable-next-line react/no-array-index-key -- rows are only ever shown in order
          <Text key={i}>
            {KINDS[cost.kind] ?? cost.kind}
            {cost.description ? ` (${cost.description})` : ""}{" "}
            {formatMoney(cost.amount, currencies)}
          </Text>
        ))}
        {po.tax && <Text>Tax {formatMoney(po.tax, currencies)}</Text>}
        <Text weight="semibold">Total {formatMoney(po.total, currencies)}</Text>
      </VStack>
      {po.note_to_supplier && (
        <VStack gap={1}>
          <Text weight="semibold">Notes</Text>
          <Text>{po.note_to_supplier}</Text>
        </VStack>
      )}
    </VStack>
  );
}
