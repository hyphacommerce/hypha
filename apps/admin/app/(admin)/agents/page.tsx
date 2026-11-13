import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { loadAgentSettings, requireStaff, sessionClient, staffNames } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { hasScope } from "@/lib/navigation";

import { AgentsTable } from "./agents-table";

/**
 * Agents: the AI clients staff connected and the autonomous ones made for a job. The
 * header and the table's toolbar render at once; the agents stream in.
 */
export default async function AgentsPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const [params, me, t] = await Promise.all([searchParams, requireStaff(), getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title={t("agents.list_title")}
          description={t("agents.list_description")}
          actions={
            <Button label={t("agents.proposals")} variant="secondary" href="/agents/proposals" />
          }
        />
        <Suspense fallback={<AgentsTable />}>
          <AgentRows params={params} canWrite={hasScope(me.scopes, "agents:write")} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function AgentRows({ params, canWrite }: { params: SearchParams; canWrite: boolean }) {
  const client = await sessionClient();
  const [me, settings, result, people] = await Promise.all([
    requireStaff(),
    loadAgentSettings(),
    client.GET("/admin/agents", {
      params: {
        query: { ...pageQuery(params), include_revoked: one(params, "revoked") === "true" },
      },
    }),
    staffNames(client),
  ]);
  return (
    <AgentsTable
      page={await listPage(result, "/agents", params)}
      settings={settings ?? undefined}
      people={Object.fromEntries(people)}
      myScopes={me.scopes}
      canWrite={canWrite}
    />
  );
}
