import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { FormSkeleton } from "@/components/skeletons";
import { type Agent, agentState, capNotice, kindLabel } from "@/lib/agents";
import {
  loadAgentSettings,
  loaded,
  perRequest,
  requireStaff,
  sessionClient,
  staffNames,
} from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import type { T } from "@/lib/messages";
import { hasScope } from "@/lib/navigation";

import { ActivityTable } from "./activity-table";
import { AgentActions } from "./agent-actions";
import { AgentEditor } from "./agent-editor";
import { AgentTabs, type AgentTab } from "./agent-tabs";

/** The way back to the list of agents. */
function back(t: T): Crumb {
  return { label: t("agents.list_title"), href: "/agents" };
}

/** The agents this request read, by ID: the header and the access form share one read. */
const agentReads = perRequest(async () => new Map<string, Promise<Agent>>());

async function loadAgent(id: string): Promise<Agent> {
  const reads = await agentReads();
  let read = reads.get(id);
  if (!read) {
    read = sessionClient().then((client) =>
      client.GET("/admin/agents/{id}", { params: { path: { id } } }).then(loaded),
    );
    reads.set(id, read);
  }
  return read;
}

/**
 * One agent: its name and grant (`?tab=` absent), or what it called (`?tab=activity`).
 * The way back and the tabs render at once; the agent streams in behind a skeleton.
 */
export default async function AgentPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<SearchParams>;
}) {
  const [{ id }, query, t] = await Promise.all([params, searchParams, getT()]);
  const tab: AgentTab = one(query, "tab") === "activity" ? "activity" : "access";
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1080}>
        <Suspense fallback={<PageHeader back={back(t)} />}>
          <AgentHeader id={id} />
        </Suspense>
        <AgentTabs id={id} tab={tab}>
          {tab === "activity" ? (
            <Suspense fallback={<ActivityTable />}>
              <Activity id={id} params={query} />
            </Suspense>
          ) : (
            <Suspense fallback={<FormSkeleton cards={3} />}>
              <Access id={id} />
            </Suspense>
          )}
        </AgentTabs>
      </VStack>
    </Section>
  );
}

async function AgentHeader({ id }: { id: string }) {
  const client = await sessionClient();
  const [t, agent, me, people, settings] = await Promise.all([
    getT(),
    loadAgent(id),
    requireStaff(),
    staffNames(client),
    loadAgentSettings(),
  ]);
  const state = agentState(t, agent);
  const person = agent.user ? (people.get(agent.user) ?? agent.user) : undefined;
  const notice = settings ? capNotice(t, settings) : null;
  const facts = [
    kindLabel(t, agent.kind),
    person && t("agents.header_acts_for", { person }),
    agent.client && t("agents.header_connected_from", { client: agent.client.name }),
  ].filter(Boolean);
  return (
    <>
      <PageHeader
        title={agent.name}
        back={back(t)}
        badges={<Badge label={state.label} variant={state.variant} />}
        description={facts.join(" · ")}
        actions={
          hasScope(me.scopes, "agents:write") &&
          !agent.revoked_at && (
            <AgentActions id={agent.id} name={agent.name} isEnabled={agent.enabled} />
          )
        }
      />
      {agent.revoked_at && (
        <Banner
          status="error"
          title={t("agents.header_revoked_title")}
          description={t("agents.header_revoked_description")}
        />
      )}
      {agent.kind === "connected" && !agent.revoked_at && (
        <Banner
          status="info"
          title={
            person
              ? t("agents.header_capped_by_person", { person })
              : t("agents.header_capped_by_staff")
          }
          description={t("agents.header_capped_description")}
        />
      )}
      {notice && <Banner status="info" title={notice.title} description={notice.description} />}
    </>
  );
}

async function Access({ id }: { id: string }) {
  const [agent, me, settings] = await Promise.all([
    loadAgent(id),
    requireStaff(),
    loadAgentSettings(),
  ]);
  return (
    <AgentEditor
      // A save starts a fresh draft from what was saved.
      key={agent.updated_at}
      agent={agent}
      cap={settings?.writes ?? agent.writes_cap}
      myScopes={me.scopes}
      canWrite={hasScope(me.scopes, "agents:write") && !agent.revoked_at}
    />
  );
}

async function Activity({ id, params }: { id: string; params: SearchParams }) {
  const client = await sessionClient();
  const result = await client.GET("/admin/agents/{id}/calls", {
    params: { path: { id }, query: pageQuery(params) },
  });
  return <ActivityTable page={await listPage(result, `/agents/${id}`, params)} />;
}
