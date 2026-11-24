"use client";

// The suppliers table: name, the contact purchase orders go to, currency, payment terms
// and lead time, searched and filtered by the API.

import { Badge } from "@astryxdesign/core/Badge";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TruckIcon } from "@heroicons/react/24/outline";

import {
  type FilterDef,
  ResourceTable,
  type View,
} from "@/components/resource-table/resource-table";
import { type Supplier, daysLabel, primaryContact, termsLabel } from "@/lib/purchasing";
import { useT } from "@/lib/use-t";

export function SuppliersTable({
  page,
  canWrite,
}: {
  /** The page of suppliers; absent while it loads (the toolbar shows meanwhile). */
  page?: { items: Supplier[]; next_cursor: string | null; error?: string };
  canWrite: boolean;
}) {
  const t = useT<"suppliers" | "purchasing">();
  const filters: FilterDef[] = [
    {
      kind: "search",
      param: "q",
      label: t("common.search"),
      placeholder: t("suppliers.search_placeholder"),
    },
    {
      kind: "select",
      param: "archived",
      label: t("common.status"),
      options: [
        { value: "false", label: t("common.active") },
        { value: "true", label: t("common.archived") },
      ],
    },
  ];
  const views: View[] = [
    { label: t("common.all"), params: {} },
    { label: t("common.active"), params: { archived: ["false"] } },
    { label: t("common.archived"), params: { archived: ["true"] } },
  ];
  const columns: TableColumn<Supplier>[] = [
    {
      key: "name",
      header: t("common.supplier"),
      width: proportional(2, { minWidth: 200 }),
      renderCell: (s) => (
        <Link href={`/suppliers/${s.id}`} weight="semibold" color="primary">
          {s.name}
        </Link>
      ),
    },
    {
      key: "contact",
      header: t("suppliers.orders_go_to"),
      width: proportional(2, { minWidth: 200 }),
      renderCell: (s) => {
        const contact = primaryContact(s);
        return (
          <Text type="supporting" color={contact ? undefined : "secondary"}>
            {contact ? (contact.email ?? contact.name) : t("suppliers.no_primary_contact")}
          </Text>
        );
      },
    },
    {
      key: "currency",
      header: t("common.currency"),
      width: pixel(100),
      renderCell: (s) => <Text>{s.currency}</Text>,
    },
    {
      key: "terms",
      header: t("suppliers.terms"),
      width: pixel(130),
      renderCell: (s) => <Text>{termsLabel(t, s.payment_terms)}</Text>,
    },
    {
      key: "lead",
      header: t("suppliers.lead_time"),
      width: pixel(110),
      renderCell: (s) => <Text>{daysLabel(t, s.lead_time_days)}</Text>,
    },
    {
      key: "status",
      header: t("common.status"),
      width: pixel(110),
      renderCell: (s) =>
        s.archived ? (
          <Badge label={t("common.archived")} variant="warning" />
        ) : (
          <Badge label={t("common.active")} variant="success" />
        ),
    },
  ];
  return (
    <ResourceTable<Supplier>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="id"
      rowHref={(s) => `/suppliers/${s.id}`}
      isSelectable={false}
      filters={filters}
      views={views}
      error={page?.error}
      empty={{
        icon: <TruckIcon width={32} />,
        title: t("suppliers.list_empty_title"),
        description: canWrite
          ? t("suppliers.list_empty_description_write")
          : t("suppliers.list_empty_description_read"),
      }}
      noResults={{ title: t("suppliers.list_no_results_title") }}
    />
  );
}
