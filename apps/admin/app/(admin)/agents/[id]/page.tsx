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
import { hasScope } from "@/lib/navigation";

import { ActivityTable } from "./activity-table";
import { AgentActions } from "./agent-actions";
import { AgentEditor } from "./agent-editor";
import { AgentTabs, type AgentTab } from "./agent-tabs";

const BACK: Crumb = { label: "Agents", href: "/agents" };

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
  const [{ id }, query] = await Promise.all([params, searchParams]);
  const tab: AgentTab = one(query, "tab") === "activity" ? "activity" : "access";
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1080}>
        <Suspense fallback={<PageHeader back={BACK} />}>
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
  const [agent, me, people, settings] = await Promise.all([
    loadAgent(id),
    requireStaff(),
    staffNames(client),
    loadAgentSettings(),
  ]);
  const state = agentState(agent);
  const person = agent.user ? (people.get(agent.user) ?? agent.user) : undefined;
  const notice = settings ? capNotice(settings) : null;
  const facts = [
    kindLabel(agent.kind),
    person && `acts for ${person}`,
    agent.client && `connected from ${agent.client.name}`,
  ].filter(Boolean);
  return (
    <>
      <PageHeader
        title={agent.name}
        back={BACK}
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
          title="This agent is revoked"
          description="Its credentials stopped for good and its pending proposals can't be approved. What it did stays in the log."
        />
      )}
      {agent.kind === "connected" && !agent.revoked_at && (
        <Banner
          status="info"
          title={`It never does more than ${person ?? "its staff member"}'s role allows`}
          description="A connected agent acts for the person who connected it: its grant is narrowed by their role at every request, and it stops if they're deactivated."
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
  return <ActivityTable page={listPage(result, `/agents/${id}`, params)} />;
}
