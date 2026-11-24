"use client";

// The proposals inbox on the shared table: views for what's waiting and what was
// decided, a filter by agent, a row per proposal.

import { Badge } from "@astryxdesign/core/Badge";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { InboxIcon } from "@heroicons/react/24/outline";
import { useMemo } from "react";

import { DateTime } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type View,
} from "@/components/resource-table/resource-table";
import { type Proposal, operationLabel, proposalStatus, shortId } from "@/lib/agents";
import { useT } from "@/lib/use-t";

const NO_NAMES: Record<string, string> = {};

export function ProposalsTable({
  page,
  agents = NO_NAMES,
}: {
  page?: { items: Proposal[]; next_cursor: string | null; error?: string };
  /** Agents' names by ID. */
  agents?: Record<string, string>;
}) {
  const t = useT<"agents" | "apps">();
  // The views: the first (no parameters) is what the signed-in staff member can approve
  // now; the server reads a missing `status` as pending and a missing `approvable` as on
  // for pending.
  const views = useMemo(
    (): View[] => [
      { label: t("agents.proposals_view_to_approve"), params: {} },
      { label: t("agents.proposals_view_all_waiting"), params: { approvable: ["false"] } },
      { label: t("agents.proposals_view_applied"), params: { status: ["applied"] } },
      { label: t("agents.proposals_view_failed"), params: { status: ["failed"] } },
      { label: t("agents.proposals_view_rejected"), params: { status: ["rejected"] } },
      { label: t("agents.proposals_view_expired"), params: { status: ["expired"] } },
      { label: t("agents.proposals_view_everything"), params: { status: ["any"] } },
    ],
    [t],
  );
  const filters: FilterDef[] = [
    {
      kind: "select",
      param: "agent",
      label: t("agents.proposals_filter_agent"),
      options: Object.entries(agents).map(([value, label]) => ({ value, label })),
    },
  ];
  const columns: TableColumn<Proposal>[] = [
    {
      key: "operation",
      header: t("agents.proposals_change"),
      renderCell: (p) => (
        <VStack gap={0.5} hAlign="start">
          <Link href={`/agents/proposals/${p.id}`} weight="semibold" color="primary">
            {operationLabel(t, p.operation, p.summary)}
          </Link>
          <Text type="supporting" color="secondary">
            {p.operation}
          </Text>
        </VStack>
      ),
    },
    {
      key: "agent",
      header: t("agents.proposals_agent"),
      width: pixel(200),
      renderCell: (p) => <Text>{agents[p.agent] ?? shortId(p.agent)}</Text>,
    },
    {
      key: "created_at",
      header: t("agents.proposals_proposed"),
      width: pixel(160),
      renderCell: (p) => <DateTime value={p.created_at} short />,
    },
    {
      key: "status",
      header: t("common.status"),
      width: pixel(190),
      renderCell: (p) => {
        const status = proposalStatus(t, p.status);
        return (
          <VStack gap={0.5} hAlign="start">
            <Badge label={status.label} variant={status.variant} />
            {p.status === "pending" && (
              <Text type="supporting" color="secondary">
                {t.rich("agents.proposals_expires", {
                  time: <DateTime key="time" value={p.expires_at} short />,
                })}
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
      views={views}
      filters={filters}
      error={page?.error}
      empty={{
        icon: <InboxIcon width={32} />,
        title: t("agents.proposals_empty_title"),
        description: t("agents.proposals_empty_description"),
      }}
    />
  );
}
