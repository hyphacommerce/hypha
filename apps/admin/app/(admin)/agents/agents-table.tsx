"use client";

// The agents list on the shared table, the deployment's cap above it, and making an
// autonomous agent (its key shown once).

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { CodeBlock } from "@astryxdesign/core/CodeBlock";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { CpuChipIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { ResourceTable, type View } from "@/components/resource-table/resource-table";
import { type Agent, type AgentSettings, agentState, capNotice, kindLabel } from "@/lib/agents";

import { createAgent } from "./actions";
import { EMPTY_GRANT, type GrantDraft, type GrantProblem, grantOf } from "./grant";
import { GrantFields } from "./grant-fields";

const NO_NAMES: Record<string, string> = {};
const NO_SCOPES: string[] = [];

const VIEWS: View[] = [
  { label: "Active", params: {} },
  { label: "With revoked", params: { revoked: ["true"] } },
];

export function AgentsTable({
  page,
  settings,
  people = NO_NAMES,
  myScopes = NO_SCOPES,
  canWrite = false,
}: {
  /** The page of agents; absent while it loads (the table shows its toolbar meanwhile). */
  page?: { items: Agent[]; next_cursor: string | null; error?: string };
  /** The operator's settings for agents; absent while the page loads. */
  settings?: AgentSettings;
  /** Staff members' names by user ID. */
  people?: Record<string, string>;
  myScopes?: readonly string[];
  canWrite?: boolean;
}) {
  const [isCreating, setIsCreating] = useState(false);
  const notice = settings ? capNotice(settings) : null;

  const columns: TableColumn<Agent>[] = [
    {
      key: "name",
      header: "Agent",
      renderCell: (a) => (
        <VStack gap={0.5} hAlign="start">
          <Link href={`/agents/${a.id}`} weight="semibold" color="primary">
            {a.name}
          </Link>
          {a.client && (
            <Text type="supporting" color="secondary">
              {a.client.name}
            </Text>
          )}
        </VStack>
      ),
    },
    {
      key: "kind",
      header: "Kind",
      width: pixel(220),
      renderCell: (a) => (
        <VStack gap={0.5} hAlign="start">
          <Badge label={kindLabel(a.kind)} />
          {a.user && (
            <Text type="supporting" color="secondary">
              For {people[a.user] ?? a.user}
            </Text>
          )}
        </VStack>
      ),
    },
    {
      key: "state",
      header: "Status",
      width: pixel(120),
      renderCell: (a) => {
        const state = agentState(a);
        return <Badge label={state.label} variant={state.variant} />;
      },
    },
    {
      key: "last_used_at",
      header: "Last used",
      width: pixel(170),
      renderCell: (a) =>
        a.last_used_at ? (
          <DateTime value={a.last_used_at} short />
        ) : (
          <Text color="secondary">Never</Text>
        ),
    },
  ];

  return (
    <VStack gap={4}>
      {notice && <Banner status="info" title={notice.title} description={notice.description} />}
      {canWrite && (
        <HStack hAlign="end">
          <Button label="Create agent" variant="primary" onClick={() => setIsCreating(true)} />
        </HStack>
      )}
      <ResourceTable<Agent>
        rows={page?.items}
        nextCursor={page?.next_cursor}
        isLoading={!page}
        columns={columns}
        idKey="id"
        rowHref={(a) => `/agents/${a.id}`}
        isSelectable={false}
        views={VIEWS}
        error={page?.error}
        empty={{
          icon: <CpuChipIcon width={32} />,
          title: "No agents yet",
          description:
            "Connect an MCP client (Claude, ChatGPT, Cursor) to the API's /mcp, or create an autonomous agent with its own key for a job.",
        }}
      />
      {isCreating && (
        <CreateAgentDialog
          cap={settings?.writes ?? "apply"}
          myScopes={myScopes}
          onClose={() => setIsCreating(false)}
        />
      )}
    </VStack>
  );
}

/** Name and grant, then the key, once. */
function CreateAgentDialog({
  cap,
  myScopes,
  onClose,
}: {
  cap: string;
  myScopes: readonly string[];
  onClose: () => void;
}) {
  const router = useRouter();
  const [name, setName] = useState("");
  const [grant, setGrant] = useState<GrantDraft>(EMPTY_GRANT);
  const [problem, setProblem] = useState<GrantProblem>();
  const [created, setCreated] = useState<{ id: string; key: string }>();
  const [isPending, startTransition] = useTransition();
  const isEmpty =
    grant.read.length === 0 &&
    Object.keys(grant.write).length === 0 &&
    grant.customerData.length === 0;

  const submit = () => {
    if (!name.trim() || isEmpty) return;
    startTransition(async () => {
      const result = await createAgent(name.trim(), grantOf(grant));
      if (result.error)
        setProblem({ error: result.error, field: result.field, scope: result.scope });
      else if (result.id && result.key) setCreated({ id: result.id, key: result.key });
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={760} purpose="form">
      <DialogHeader
        title={created ? `${name.trim()} is ready` : "Create agent"}
        subtitle={
          created
            ? "Copy its key now: you won't see it again."
            : "An autonomous agent works on its own, for a job, with the access you give it here."
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {created ? (
          <>
            <CodeBlock code={created.key} title="Agent key" isWrapped width="100%" />
            <Banner
              status="warning"
              title="This is the only time the key is shown"
              description="Give it to the agent's MCP client as its bearer token for the API's /mcp. It works nowhere else. Lost it? Revoke the agent and create another."
            />
          </>
        ) : (
          <>
            {problem && !problem.field && <Banner status="error" title={problem.error} />}
            {problem?.field && problem.field !== "name" && (
              <Banner status="error" title="The grant can't be saved" description={problem.error} />
            )}
            <TextInput
              label="Name"
              description="Shown in the inbox beside what it proposes, so say what it's for."
              placeholder="Nightly restock"
              value={name}
              onChange={setName}
              status={
                problem?.field === "name" ? { type: "error", message: problem.error } : undefined
              }
            />
            <GrantFields
              value={grant}
              onChange={setGrant}
              myScopes={myScopes}
              cap={cap}
              problem={problem}
            />
          </>
        )}
        <HStack gap={2} hAlign="end">
          {created ? (
            <Button
              label="Done"
              variant="primary"
              onClick={() => router.push(`/agents/${created.id}`)}
            />
          ) : (
            <>
              <Button label="Cancel" variant="ghost" onClick={onClose} />
              <Button
                label="Create agent"
                variant="primary"
                isLoading={isPending}
                isDisabled={!name.trim() || isEmpty}
                onClick={submit}
              />
            </>
          )}
        </HStack>
      </VStack>
    </Dialog>
  );
}
