"use client";

// The customer list's table (design note 0024, §3).

import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { UsersIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";

import { DateTime } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type SortDef,
  type SortOption,
} from "@/components/resource-table/resource-table";


type Customer = AdminSchemas["CustomerSummaryResponse"];

const SORT_OPTIONS: SortOption[] = [
  { value: "newest", label: "Newest" },
  { value: "oldest", label: "Oldest" },
  { value: "name_asc", label: "Name A–Z" },
  { value: "name_desc", label: "Name Z–A" },
  { value: "email_asc", label: "Email A–Z" },
  { value: "email_desc", label: "Email Z–A" },
];

const SORTS: Record<string, SortDef> = {
  name: { asc: "name_asc", desc: "name_desc" },
  email: { asc: "email_asc", desc: "email_desc" },
  created: { asc: "oldest", desc: "newest" },
};

const FILTERS: FilterDef[] = [
  { kind: "search", param: "q", label: "Search", placeholder: "Search email, name or phone" },
];

/** "Ana Horvat", or nothing for a customer known only by email. */
export function customerName(c: { first_name?: string | null; last_name?: string | null }) {
  return [c.first_name, c.last_name].filter(Boolean).join(" ");
}

export function CustomersTable({
  page,
}: {
  page: { items: Customer[]; next_cursor: string | null; error?: string };
}) {
  const columns: TableColumn<Customer>[] = [
    {
      key: "name",
      header: "Name",
      width: proportional(2, { minWidth: 200 }),
      renderCell: (c) => (
        <VStack gap={0.5}>
          <Link href={`/customers/${c.id}`} weight="medium">
            {customerName(c) || c.email}
          </Link>
          {customerName(c) && (
            <Text type="supporting" color="secondary">
              {c.email}
            </Text>
          )}
        </VStack>
      ),
    },
    { key: "email", header: "Email", width: proportional(2) },
    {
      key: "phone",
      header: "Phone",
      renderCell: (c) => c.phone ?? <Text color="secondary">—</Text>,
    },
    { key: "address_count", header: "Addresses", width: pixel(100), align: "end" },
    {
      key: "created",
      header: "Since",
      width: pixel(190),
      renderCell: (c) => <DateTime value={c.created_at} short />,
    },
  ];
  return (
    <ResourceTable<Customer>
      rows={page.items}
      nextCursor={page.next_cursor}
      columns={columns}
      idKey="id"
      rowHref={(c) => `/customers/${c.id}`}
      filters={FILTERS}
      sorts={SORTS}
      sortOptions={SORT_OPTIONS}
      defaultSort="newest"
      error={page.error}
      empty={{
        icon: <UsersIcon width={32} />,
        title: "No customers yet",
        description: "Checkout adds a customer for each new email; they show up here.",
      }}
      noResults={{ title: "No customers match", description: "Search looks at emails, names and phones." }}
    />
  );
}
