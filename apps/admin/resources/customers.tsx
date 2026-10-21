"use client";

// The customer list's table (design note 0024, §3), with the labels field's column and
// bulk actions when the merchant has it (design note 0032, CD1).

import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { Token } from "@astryxdesign/core/Token";
import { UsersIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useState } from "react";

import { customFieldFilters } from "@/components/custom-fields/filters";
import { DateTime } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type SortDef,
  type SortOption,
} from "@/components/resource-table/resource-table";
import { type Definition, filterParam } from "@/lib/custom-fields";
import { customerName } from "@/lib/format";
import { textItems } from "@/lib/well-known-fields";

import type { Group } from "@/app/(admin)/customers/groups/groups";

import { type BulkOutcome, BulkOutcomeBanner, CustomerBulkActions } from "./bulk";

/** No filterable custom fields. */
const NO_FIELDS: Definition[] = [];
/** No customer groups. */
const NO_GROUPS: Group[] = [];

/** Labels shown in a row before "+n". */
const SHOWN_LABELS = 3;

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
  {
    kind: "search",
    param: "q",
    label: "Search",
    placeholder: "Search customers by email, name or phone",
  },
];

export function CustomersTable({
  page,
  fieldFilters = NO_FIELDS,
  labels,
  groups = NO_GROUPS,
  canLabel = false,
}: {
  page: { items: Customer[]; next_cursor: string | null; error?: string };
  /** Filterable custom fields (design note 0033, §7), offered as filters. */
  fieldFilters?: Definition[];
  /** The well-known labels field, when the merchant has it: a column and bulk actions. */
  labels?: Definition;
  /** The store's customer groups (design note 0032, §6): a filter and bulk actions. */
  groups?: Group[];
  /** `customers:write`: labels and groups can be changed on the selection. */
  canLabel?: boolean;
}) {
  const [outcome, setOutcome] = useState<BulkOutcome>();
  const groupFilter: FilterDef[] =
    groups.length > 0
      ? [
          {
            kind: "select",
            param: "group",
            label: "Group",
            options: groups.map((g) => ({ value: g.id, label: g.name })),
          },
        ]
      : [];
  const filters = [...FILTERS, ...groupFilter, ...customFieldFilters(fieldFilters)];
  const columns: TableColumn<Customer>[] = [
    {
      key: "name",
      header: "Customer",
      width: proportional(2, { minWidth: 200 }),
      renderCell: (c) => (
        <VStack gap={0.5}>
          <Link href={`/customers/${c.id}`} weight="semibold" color="primary">
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
    ...(labels
      ? [
          {
            key: "labels",
            header: labels.name,
            width: proportional(1, { minWidth: 160 }),
            renderCell: (c: Customer) => {
              const items = textItems(c.custom_fields?.[labels.field]);
              if (items.length === 0) return <Text color="secondary">—</Text>;
              return (
                <HStack gap={1} wrap="wrap">
                  {items.slice(0, SHOWN_LABELS).map((label) => (
                    <Token
                      key={label}
                      label={label}
                      size="sm"
                      href={`/customers?${encodeURIComponent(filterParam(labels))}=${encodeURIComponent(label)}`}
                    />
                  ))}
                  {items.length > SHOWN_LABELS && (
                    <Text type="supporting" color="secondary">
                      +{items.length - SHOWN_LABELS}
                    </Text>
                  )}
                </HStack>
              );
            },
          } satisfies TableColumn<Customer>,
        ]
      : []),
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
  const bulk = canLabel && (labels !== undefined || groups.length > 0);
  return (
    <VStack gap={3}>
      {outcome && (
        <BulkOutcomeBanner
          outcome={outcome}
          noun="customer"
          onClose={() => setOutcome(undefined)}
        />
      )}
      <ResourceTable<Customer>
        rows={page.items}
        nextCursor={page.next_cursor}
        columns={columns}
        idKey="id"
        rowHref={(c) => `/customers/${c.id}`}
        rowLabel={(c) => customerName(c) || c.email}
        isSelectable={bulk}
        renderBulkActions={
          bulk
            ? (selected, clear) => (
                <CustomerBulkActions
                  selected={selected}
                  clear={clear}
                  labels={labels}
                  groups={groups}
                  onDone={setOutcome}
                />
              )
            : undefined
        }
        filters={filters}
        sorts={SORTS}
        sortOptions={SORT_OPTIONS}
        defaultSort="newest"
        error={page.error}
        empty={{
          icon: <UsersIcon width={32} />,
          title: "No customers yet",
          description: "Checkout adds a customer for each new email; they show up here.",
        }}
        noResults={{
          title: "No customers match",
          description: "Search looks at emails, names and phones.",
        }}
      />
    </VStack>
  );
}
