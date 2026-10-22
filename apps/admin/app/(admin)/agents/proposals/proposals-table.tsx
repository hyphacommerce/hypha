"use client";

// The proposals inbox on the shared table: views for what's waiting and what was
// decided, a filter by agent, a row per proposal.

import { Badge } from "@astryxdesign/core/Badge";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { InboxIcon } from "@heroicons/react/24/outline";

import { DateTime } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type View,
} from "@/components/resource-table/resource-table";
import { type Proposal, operationLabel, proposalStatus, shortId } from "@/lib/agents";

const NO_NAMES: Record<string, string> = {};

/**
 * The views: the first (no parameters) is what the signed-in staff member can approve
 * now; the server reads a missing `status` as pending and a missing `approvable` as on
 * for pending.
 */
const VIEWS: View[] = [
  { label: "To approve", params: {} },
  { label: "All waiting", params: { approvable: ["false"] } },
  { label: "Applied", params: { status: ["applied"] } },
  { label: "Failed", params: { status: ["failed"] } },
  { label: "Rejected", params: { status: ["rejected"] } },
  { label: "Expired", params: { status: ["expired"] } },
  { label: "Everything", params: { status: ["any"] } },
];

export function ProposalsTable({
  page,
  agents = NO_NAMES,
}: {
  page?: { items: Proposal[]; next_cursor: string | null; error?: string };
  /** Agents' names by ID. */
  agents?: Record<string, string>;
}) {
  const filters: FilterDef[] = [
    {
      kind: "select",
      param: "agent",
      label: "Agent",
      options: Object.entries(agents).map(([value, label]) => ({ value, label })),
    },
  ];
  const columns: TableColumn<Proposal>[] = [
    {
      key: "operation",
      header: "Change",
      renderCell: (p) => (
        <VStack gap={0.5} hAlign="start">
          <Link href={`/agents/proposals/${p.id}`} weight="semibold" color="primary">
            {operationLabel(p.operation, p.summary)}
          </Link>
          <Text type="supporting" color="secondary">
            {p.operation}
          </Text>
        </VStack>
      ),
    },
    {
      key: "agent",
      header: "Agent",
      width: pixel(200),
      renderCell: (p) => <Text>{agents[p.agent] ?? shortId(p.agent)}</Text>,
    },
    {
      key: "created_at",
      header: "Proposed",
      width: pixel(160),
      renderCell: (p) => <DateTime value={p.created_at} short />,
    },
    {
      key: "status",
      header: "Status",
      width: pixel(190),
      renderCell: (p) => {
        const status = proposalStatus(p.status);
        return (
          <VStack gap={0.5} hAlign="start">
            <Badge label={status.label} variant={status.variant} />
            {p.status === "pending" && (
              <Text type="supporting" color="secondary">
                Expires <DateTime value={p.expires_at} short />
              </Text>
            )}
          </VStack>
        );
      },
    },
  ];
  return (
    <ResourceTable<Proposal>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="id"
      rowHref={(p) => `/agents/proposals/${p.id}`}
      isSelectable={false}
      views={VIEWS}
      filters={filters}
      error={page?.error}
      empty={{
        icon: <InboxIcon width={32} />,
        title: "No proposals here",
        description:
          "When an agent proposes a change you can approve, it waits here with a preview of exactly what it would do.",
      }}
    />
  );
}
