"use client";

// An agent's call log: every operation it called, reads too, newest first. No bodies:
// they may hold personal data.

import { Badge } from "@astryxdesign/core/Badge";
import { VStack } from "@astryxdesign/core/Layout";
import { type TableColumn, pixel } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ClockIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";

import { DateTime } from "@/components/date-time";
import { ResourceTable } from "@/components/resource-table/resource-table";
import { modeLabel, operationLabel } from "@/lib/agents";

type Call = AdminSchemas["AgentCallResponse"];

function statusVariant(status: number): "success" | "warning" | "error" {
  if (status < 300) return "success";
  return status < 500 ? "warning" : "error";
}

export function ActivityTable({
  page,
}: {
  page?: { items: Call[]; next_cursor: string | null; error?: string };
}) {
  const columns: TableColumn<Call>[] = [
    {
      key: "operation",
      header: "Operation",
      renderCell: (c) => (
        <VStack gap={0.5}>
          <Text weight="medium">{operationLabel(c.operation, c.summary)}</Text>
          <Text type="code">{c.operation}</Text>
        </VStack>
      ),
    },
    {
      key: "mode",
      header: "Mode",
      width: pixel(100),
      renderCell: (c) => <Badge label={modeLabel(c.mode)} />,
    },
    {
      key: "status",
      header: "Status",
      width: pixel(220),
      renderCell: (c) => (
        <VStack gap={0.5} hAlign="start">
          <Badge label={String(c.status)} variant={statusVariant(c.status)} />
          {c.error_code && <Text type="code">{c.error_code}</Text>}
        </VStack>
      ),
    },
    {
      key: "duration_ms",
      header: "Took",
      width: pixel(90),
      align: "end",
      renderCell: (c) => <Text hasTabularNumbers>{c.duration_ms} ms</Text>,
    },
    {
      key: "created_at",
      header: "When",
      width: pixel(170),
      renderCell: (c) => <DateTime value={c.created_at} short />,
    },
  ];
  return (
    <ResourceTable<Call>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="id"
      isSelectable={false}
      error={page?.error}
      empty={{
        icon: <ClockIcon width={32} />,
        title: "No calls yet",
        description: "Every operation the agent calls shows up here, reads too, kept for 30 days.",
      }}
    />
  );
}
