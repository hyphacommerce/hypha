"use client";

// The customer list's table, with the labels field's column and bulk actions when the
// merchant has it.

import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { Token } from "@astryxdesign/core/Token";
import { UsersIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import type { KeyIn } from "@/lib/messages";
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
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";
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

/** Every order the list offers, as the API names them. */
const SORT_OPTIONS: { value: string; label: KeyIn<"common" | "customers"> }[] = [
  { value: "newest", label: "common.sort_newest" },
  { value: "oldest", label: "common.sort_oldest" },
  { value: "name_asc", label: "common.sort_name_asc" },
  { value: "name_desc", label: "common.sort_name_desc" },
  { value: "email_asc", label: "customers.list_sort_email_asc" },
  { value: "email_desc", label: "customers.list_sort_email_desc" },
];
const SORTS: Record<string, SortDef> = {
  name: { asc: "name_asc", desc: "name_desc" },
  email: { asc: "email_asc", desc: "email_desc" },
  created: { asc: "oldest", desc: "newest" },
};

export function CustomersTable({
  page,
  fieldFilters = NO_FIELDS,
  labels,
  groups = NO_GROUPS,
  canLabel = false,
}: {
  /** The page of rows; absent while it loads (the table shows its toolbar meanwhile). */
  page?: { items: Customer[]; next_cursor: string | null; error?: string };
  /** Filterable custom fields, offered as filters. */
  fieldFilters?: Definition[];
  /** The well-known labels field, when the merchant has it: a column and bulk actions. */
  labels?: Definition;
  /** The store's customer groups: a filter and bulk actions. */
  groups?: Group[];
  /** `customers:write`: labels and groups can be changed on the selection. */
  canLabel?: boolean;
}) {
  const t = useT<"customers">();
  const format = useFormat();
  const [outcome, setOutcome] = useState<BulkOutcome>();
  const sortOptions: SortOption[] = SORT_OPTIONS.map(({ value, label }) => ({
    value,
    label: t(label),
  }));
  const search: FilterDef = {
    kind: "search",
    param: "q",
    label: t("common.search"),
    placeholder: t("customers.list_search_placeholder"),
  };
  const groupFilter: FilterDef[] =
    groups.length > 0
      ? [
          {
            kind: "select",
            param: "group",
            label: t("customers.group"),
            options: groups.map((g) => ({ value: g.id, label: g.name })),
          },
        ]
      : [];
  const filters = [search, ...groupFilter, ...customFieldFilters(fieldFilters, t)];
  const columns: TableColumn<Customer>[] = [
    {
      key: "name",
      header: t("common.customer"),
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
              if (items.length === 0) {
                return <Text color="secondary">{t("common.empty_value")}</Text>;
              }
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
                      +{format.number(items.length - SHOWN_LABELS)}
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
      header: t("common.phone"),
      renderCell: (c) => c.phone ?? <Text color="secondary">{t("common.empty_value")}</Text>,
    },
    {
      key: "address_count",
      header: t("customers.list_column_addresses"),
      width: pixel(100),
      align: "end",
    },
    {
      key: "created",
      header: t("customers.list_column_since"),
      width: pixel(190),
      renderCell: (c) => <DateTime value={c.created_at} short />,
    },
  ];
  const bulk = canLabel && (labels !== undefined || groups.length > 0);
  return (
    <VStack gap={3}>
      {outcome && <BulkOutcomeBanner outcome={outcome} onClose={() => setOutcome(undefined)} />}
      <ResourceTable<Customer>
        rows={page?.items}
        nextCursor={page?.next_cursor}
        isLoading={!page}
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
        sortOptions={sortOptions}
        defaultSort="newest"
        error={page?.error}
        empty={{
          icon: <UsersIcon width={32} />,
          title: t("customers.list_empty_title"),
          description: t("customers.list_empty_description"),
        }}
        noResults={{
          title: t("customers.list_no_results_title"),
          description: t("customers.list_no_results_description"),
        }}
      />
    </VStack>
  );
}
