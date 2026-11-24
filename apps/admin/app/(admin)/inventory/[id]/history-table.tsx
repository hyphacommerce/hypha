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

import { DateTime } from "@/components/date-time";
import { type FilterDef, ResourceTable } from "@/components/resource-table/resource-table";
import { actorLabel } from "@/lib/apps";
import { STOCK_REASONS, reasonLabel } from "@/lib/inventory";
import type { TFor } from "@/lib/messages";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

type Movement = AdminSchemas["StockMovementResponse"];

const NO_NAMES: Record<string, string> = {};

/** What caused it, linked where the admin shows it. */
function Source({ source, t }: { source: Movement["source"]; t: TFor<"inventory"> }) {
  switch (source.type) {
    case "fulfillment":
      return (
        <Link href={`/orders/${source.order}`} color="primary">
          {t("inventory.source_fulfillment")}
        </Link>
      );
    case "receipt":
      return (
        <Link href={`/purchase-orders/${source.purchase_order}`} color="primary">
          {t("inventory.source_receipt")}
        </Link>
      );
    case "purchase_order":
      return (
        <Link href={`/purchase-orders/${source.purchase_order}`} color="primary">
          {t("inventory.source_purchase_order")}
        </Link>
      );
    case "transfer":
      return (
        <Link href={`/transfers/${source.transfer}`} color="primary">
          {t("inventory.source_transfer")}
        </Link>
      );
    case "migration":
      return <Text color="secondary">{t("inventory.source_migration")}</Text>;
    case "manual":
      return <Text color="secondary">{t("inventory.source_manual")}</Text>;
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
  const t = useT<"inventory" | "apps">();
  const format = useFormat();
  const cost = (m: Movement) => {
    if (m.reason === "cost_adjustment") {
      const none = t("inventory.history_cost_none");
      const before = m.unit_cost ? format.unitCost(m.unit_cost) : none;
      const after = m.average_after ? format.unitCost(m.average_after) : none;
      return `${before} → ${after}`;
    }
    return m.unit_cost ? format.unitCost(m.unit_cost) : "—";
  };
  const filters: FilterDef[] = [
    {
      kind: "select",
      param: "location",
      label: t("common.location"),
      options: Object.entries(locations).map(([value, label]) => ({ value, label })),
    },
    {
      kind: "select",
      param: "reason",
      label: t("inventory.reason"),
      options: STOCK_REASONS.map((value) => ({ value, label: reasonLabel(t, value) })),
    },
  ];
  const columns: TableColumn<Movement>[] = [
    {
      key: "at",
      header: t("inventory.history_when"),
      width: pixel(170),
      renderCell: (m) => <DateTime value={m.at} short />,
    },
    {
      key: "reason",
      header: t("inventory.reason"),
      renderCell: (m) => (
        <VStack gap={0.5} hAlign="start">
          <Badge label={reasonLabel(t, m.reason)} />
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
      header: t("common.location"),
      width: pixel(160),
      renderCell: (m) => (
        <Text>{m.location ? (locations[m.location] ?? t("inventory.removed_location")) : "—"}</Text>
      ),
    },
    {
      key: "delta",
      header: t("inventory.change"),
      width: pixel(90),
      align: "end",
      renderCell: (m) => (
        <Text hasTabularNumbers color={m.delta < 0 ? "secondary" : undefined}>
          {format.number(m.delta, { signDisplay: "exceptZero" })}
        </Text>
      ),
    },
    {
      key: "after",
      header: t("inventory.history_after"),
      width: pixel(120),
      align: "end",
      renderCell: (m) => (
        <Text hasTabularNumbers>{m.after == null ? "—" : format.number(m.after)}</Text>
      ),
    },
    ...(showCost
      ? [
          {
            key: "unit_cost",
            header: t("inventory.unit_cost"),
            width: pixel(170),
            align: "end",
            renderCell: (m: Movement) => <Text hasTabularNumbers>{cost(m)}</Text>,
          } satisfies TableColumn<Movement>,
        ]
      : []),
    {
      key: "source",
      header: t("inventory.history_source"),
      width: pixel(170),
      renderCell: (m) => (
        <VStack gap={0.5} hAlign="start">
          <Source source={m.source} t={t} />
          <Text type="supporting" color="secondary">
            {actorLabel(t, m.actor)}
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
        title: t("inventory.history_empty_title"),
        description: t("inventory.history_empty_description"),
      }}
      noResults={{
        title: t("inventory.no_results_title"),
        description: t("inventory.history_no_results_description"),
      }}
    />
  );
}
