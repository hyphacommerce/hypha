import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { Suspense } from "react";

import { DetailSkeleton } from "@/components/skeletons";
import { getFormat } from "@/lib/get-format";
import { loadStore, loaded, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { costKindLabel, poNumber, termsLabel } from "@/lib/purchasing";

import { PrintButton } from "./print-button";

/**
 * A purchase order as a document to print or save as a PDF: what the supplier gets by
 * email, in the order's currency, with where the goods go. It's written in the staff
 * member's language, like the rest of the admin.
 */
export default async function PrintPurchaseOrderPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const [{ id }, t] = await Promise.all([params, getT()]);
  return (
    <Section padding={6}>
      <Suspense
        fallback={
          <DetailSkeleton
            back={[{ label: t("purchasing.print_back_label"), href: `/purchase-orders/${id}` }]}
          />
        }
      >
        <PrintView id={id} />
      </Suspense>
    </Section>
  );
}

async function PrintView({ id }: { id: string }) {
  const client = await sessionClient();
  const [po, store, t, format] = await Promise.all([
    client.GET("/admin/purchase-orders/{id}", { params: { path: { id } } }).then(loaded),
    loadStore(),
    getT(),
    getFormat(),
  ]);
  const [supplier, location] = await Promise.all([
    client.GET("/admin/suppliers/{id}", { params: { path: { id: po.supplier } } }),
    po.destination
      ? client.GET("/admin/locations/{id}", { params: { path: { id: po.destination } } })
      : undefined,
  ]);
  const address = location?.data?.address;
  const costs = po.costs.filter((c) => c.amount.currency === po.currency);
  const terms = po.payment_terms;
  return (
    <VStack gap={5} maxWidth={860} className="po-print">
      <HStack gap={2} className="no-print">
        <Button label={t("common.back")} variant="ghost" href={`/purchase-orders/${po.id}`} />
        <PrintButton />
      </HStack>
      <HStack hAlign="between" vAlign="start" gap={4} wrap="wrap">
        <VStack gap={1}>
          <Text type="display-2" as="h1">
            {t("purchasing.print_title", { number: poNumber(po.number) })}
          </Text>
          {po.revision > 0 && (
            <Text color="secondary">
              {t("purchasing.print_revision", { revision: po.revision })}
            </Text>
          )}
          <Text>{store?.name}</Text>
          {store?.contact_email && <Text color="secondary">{store.contact_email}</Text>}
        </VStack>
        <VStack gap={1}>
          {po.ordered_at && (
            <Text>{t("purchasing.print_ordered", { date: format.date(po.ordered_at) })}</Text>
          )}
          {po.expected_at && (
            <Text>{t("purchasing.print_expected", { date: format.date(po.expected_at) })}</Text>
          )}
          {po.supplier_reference && (
            <Text>{t("purchasing.print_reference", { reference: po.supplier_reference })}</Text>
          )}
          {terms && (
            <Text>
              {terms.note
                ? t("purchasing.print_terms_note", {
                    terms: termsLabel(t, terms),
                    note: terms.note,
                  })
                : t("purchasing.print_terms", { terms: termsLabel(t, terms) })}
            </Text>
          )}
        </VStack>
      </HStack>
      <HStack gap={8} vAlign="start" wrap="wrap">
        <VStack gap={1}>
          <Text weight="semibold">{t("common.supplier")}</Text>
          <Text>{po.supplier_name ?? supplier.data?.name}</Text>
          {supplier.data?.tax_number && <Text color="secondary">{supplier.data.tax_number}</Text>}
        </VStack>
        {location?.data && (
          <VStack gap={1}>
            <Text weight="semibold">{t("purchasing.print_ship_to")}</Text>
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
            <th>{t("purchasing.print_item")}</th>
            <th>{t("purchasing.print_your_code")}</th>
            <th>{t("purchasing.print_our_sku")}</th>
            <th className="amount">{t("common.quantity")}</th>
            <th className="amount">{t("purchasing.unit_cost")}</th>
            <th className="amount">{t("common.total")}</th>
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
              <td className="amount">{format.number(line.quantity - line.cancelled)}</td>
              <td className="amount">{format.unitCost(line.unit_cost)}</td>
              <td className="amount">{format.money(line.total)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <VStack gap={1} hAlign="end">
        <Text>{t("purchasing.print_subtotal", { amount: format.money(po.subtotal) })}</Text>
        {costs.map((cost, i) => (
          // Cost rows have no ID; their order is the order's.
          // oxlint-disable-next-line react/no-array-index-key -- rows are only ever shown in order
          <Text key={i}>
            {cost.description
              ? t("purchasing.print_cost_described", {
                  kind: costKindLabel(t, cost.kind),
                  description: cost.description,
                  amount: format.money(cost.amount),
                })
              : t("purchasing.print_cost", {
                  kind: costKindLabel(t, cost.kind),
                  amount: format.money(cost.amount),
                })}
          </Text>
        ))}
        {po.tax && <Text>{t("purchasing.print_tax", { amount: format.money(po.tax) })}</Text>}
        <Text weight="semibold">
          {t("purchasing.print_total", { amount: format.money(po.total) })}
        </Text>
      </VStack>
      {po.note_to_supplier && (
        <VStack gap={1}>
          <Text weight="semibold">{t("common.notes")}</Text>
          <Text>{po.note_to_supplier}</Text>
        </VStack>
      )}
    </VStack>
  );
}
