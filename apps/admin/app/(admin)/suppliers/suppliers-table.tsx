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

const FILTERS: FilterDef[] = [
  { kind: "search", param: "q", label: "Search", placeholder: "Search supplier names" },
  {
    kind: "select",
    param: "archived",
    label: "Status",
    options: [
      { value: "false", label: "Active" },
      { value: "true", label: "Archived" },
    ],
  },
];

const VIEWS: View[] = [
  { label: "All", params: {} },
  { label: "Active", params: { archived: ["false"] } },
  { label: "Archived", params: { archived: ["true"] } },
];

const COLUMNS: TableColumn<Supplier>[] = [
  {
    key: "name",
    header: "Supplier",
    width: proportional(2, { minWidth: 200 }),
    renderCell: (s) => (
      <Link href={`/suppliers/${s.id}`} weight="semibold" color="primary">
        {s.name}
      </Link>
    ),
  },
  {
    key: "contact",
    header: "Orders go to",
    width: proportional(2, { minWidth: 200 }),
    renderCell: (s) => {
      const contact = primaryContact(s);
      return (
        <Text type="supporting" color={contact ? undefined : "secondary"}>
          {contact ? (contact.email ?? contact.name) : "No primary contact"}
        </Text>
      );
    },
  },
  {
    key: "currency",
    header: "Currency",
    width: pixel(100),
    renderCell: (s) => <Text>{s.currency}</Text>,
  },
  {
    key: "terms",
    header: "Terms",
    width: pixel(130),
    renderCell: (s) => <Text>{termsLabel(s.payment_terms)}</Text>,
  },
  {
    key: "lead",
    header: "Lead time",
    width: pixel(110),
    renderCell: (s) => <Text>{daysLabel(s.lead_time_days)}</Text>,
  },
  {
    key: "status",
    header: "Status",
    width: pixel(110),
    renderCell: (s) =>
      s.archived ? (
        <Badge label="Archived" variant="warning" />
      ) : (
        <Badge label="Active" variant="success" />
      ),
  },
];

export function SuppliersTable({
  page,
  canWrite,
}: {
  /** The page of suppliers; absent while it loads (the toolbar shows meanwhile). */
  page?: { items: Supplier[]; next_cursor: string | null; error?: string };
  canWrite: boolean;
}) {
  return (
    <ResourceTable<Supplier>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={COLUMNS}
      idKey="id"
      rowHref={(s) => `/suppliers/${s.id}`}
      isSelectable={false}
      filters={FILTERS}
      views={VIEWS}
      error={page?.error}
      empty={{
        icon: <TruckIcon width={32} />,
        title: "No suppliers yet",
        description: canWrite
          ? "Add the businesses you buy stock from, with their contacts, terms and prices."
          : "Suppliers the store buys from show here.",
      }}
      noResults={{ title: "No suppliers match these filters" }}
    />
  );
}
