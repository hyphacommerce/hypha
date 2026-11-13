import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one, oneOf } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { PROPOSAL_STATUSES } from "@/lib/agents";
import { sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";

import { ProposalsTable } from "./proposals-table";

/**
 * The proposals inbox: what agents asked to write, waiting for a person. By default the
 * pending ones the signed-in staff member can approve (`status` absent is `pending`,
 * `approvable` absent is on for pending); `status=any` lists every status.
 */
export default async function ProposalsPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const [params, t] = await Promise.all([searchParams, getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title={t("agents.proposals_title")}
          back={{ label: t("agents.list_title"), href: "/agents" }}
          description={t("agents.proposals_description")}
        />
        <Suspense fallback={<ProposalsTable />}>
          <ProposalRows params={params} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function ProposalRows({ params }: { params: SearchParams }) {
  const client = await sessionClient();
  const chosen = oneOf(params, "status", [...PROPOSAL_STATUSES, "any"]);
  const status = chosen === "any" ? undefined : (chosen ?? "pending");
  const approvable = one(params, "approvable");
  const [result, agents] = await Promise.all([
    client.GET("/admin/agent-proposals", {
      params: {
        query: {
          ...pageQuery(params),
          status,
          agent: one(params, "agent"),
          approvable: approvable === "true" || (approvable === undefined && status === "pending"),
        },
      },
    }),
    client.GET("/admin/agents", { params: { query: { limit: 100, include_revoked: true } } }),
  ]);
  return (
    <ProposalsTable
      page={await listPage(result, "/agents/proposals", params)}
      agents={Object.fromEntries((agents.data?.items ?? []).map((a) => [a.id, a.name]))}
    />
  );
}
