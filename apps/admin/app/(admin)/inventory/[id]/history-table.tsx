"use client";

// An inventory item's stock history: every change to a count, newest first, with why,
// what caused it and who made it.

import { Badge } from "@astryxdesign/core/Badge";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ClockIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { useCurrencies } from "@/components/currencies";
import { DateTime } from "@/components/date-time";
import { type FilterDef, ResourceTable } from "@/components/resource-table/resource-table";
import { actorLabel } from "@/lib/apps";
import { formatUnitCost } from "@/lib/format";
import { REASON_LABELS } from "@/lib/inventory";

type Movement = AdminSchemas["StockMovementResponse"];

const NO_NAMES: Record<string, string> = {};

/** What caused it, linked where the admin shows it. */
function Source({ source }: { source: Movement["source"] }) {
  switch (source.type) {
    case "fulfillment":
      return (
        <Link href={`/orders/${source.order}`} color="primary">
          Fulfilment
        </Link>
      );
    case "receipt":
      return <Text>Purchase order receipt</Text>;
    case "purchase_order":
      return <Text>Purchase order</Text>;
    case "transfer":
      return <Text>Transfer</Text>;
    case "migration":
      return <Text color="secondary">When history began</Text>;
    case "manual":
      return <Text color="secondary">By hand</Text>;
  }
}

export function HistoryTable({
  page,
  locations = NO_NAMES,
  showCost = false,
}: {
  page?: { items: Movement[]; next_cursor: string | null; error?: string };
  /** Locations' names by ID. */
  locations?: Record<string, string>;
  /** Whether the staff member sees cost (`purchasing:read`): a cost column then. */
  showCost?: boolean;
}) {
  const currencies = useCurrencies();
  const cost = (m: Movement) => {
    if (m.reason === "cost_adjustment") {
      const before = m.unit_cost ? formatUnitCost(m.unit_cost, currencies) : "none";
      const after = m.average_after ? formatUnitCost(m.average_after, currencies) : "none";
      return `${before} → ${after}`;
    }
    return m.unit_cost ? formatUnitCost(m.unit_cost, currencies) : "—";
  };
  const filters: FilterDef[] = [
    {
      kind: "select",
      param: "location",
      label: "Location",
      options: Object.entries(locations).map(([value, label]) => ({ value, label })),
    },
    {
      kind: "select",
      param: "reason",
      label: "Reason",
      options: Object.entries(REASON_LABELS).map(([value, label]) => ({ value, label })),
    },
  ];
  const columns: TableColumn<Movement>[] = [
    {
      key: "at",
      header: "When",
      width: pixel(170),
      renderCell: (m) => <DateTime value={m.at} short />,
    },
    {
      key: "reason",
      header: "Reason",
      renderCell: (m) => (
        <VStack gap={0.5} hAlign="start">
          <Badge label={REASON_LABELS[m.reason] ?? m.reason} />
          {m.note && (
            <Text type="supporting" color="secondary">
              {m.note}
            </Text>
          )}
        </VStack>
      ),
    },
    {
      key: "location",
      header: "Location",
      width: pixel(160),
      renderCell: (m) => (
        <Text>{m.location ? (locations[m.location] ?? "Removed location") : "—"}</Text>
      ),
    },
    {
      key: "delta",
      header: "Change",
      width: pixel(90),
      align: "end",
      renderCell: (m) => (
        <Text hasTabularNumbers color={m.delta < 0 ? "secondary" : undefined}>
          {m.delta > 0 ? `+${m.delta}` : m.delta}
        </Text>
      ),
    },
    {
      key: "after",
      header: "On hand after",
      width: pixel(120),
      align: "end",
      renderCell: (m) => <Text hasTabularNumbers>{m.after ?? "—"}</Text>,
    },
    ...(showCost
      ? [
          {
            key: "unit_cost",
            header: "Unit cost",
            width: pixel(170),
            align: "end",
            renderCell: (m: Movement) => <Text hasTabularNumbers>{cost(m)}</Text>,
          } satisfies TableColumn<Movement>,
        ]
      : []),
    {
      key: "source",
      header: "Source",
      width: pixel(170),
      renderCell: (m) => (
        <VStack gap={0.5} hAlign="start">
          <Source source={m.source} />
          <Text type="supporting" color="secondary">
            {actorLabel(m.actor)}
          </Text>
        </VStack>
      ),
    },
  ];
  return (
    <ResourceTable<Movement>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="position"
      isSelectable={false}
      filters={filters}
      error={page?.error}
      empty={{
        icon: <ClockIcon width={32} />,
        title: "No changes yet",
        description: "Every count, adjustment, sale and delivery shows here, kept for good.",
      }}
      noResults={{
        title: "Nothing here",
        description: "No change matches these filters.",
      }}
    />
  );
}
