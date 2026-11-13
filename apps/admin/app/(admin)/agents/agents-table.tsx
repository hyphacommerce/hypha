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
import { useMemo, useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { ResourceTable, type View } from "@/components/resource-table/resource-table";
import { type Agent, type AgentSettings, agentState, capNotice, kindLabel } from "@/lib/agents";
import { useT } from "@/lib/use-t";

import { createAgent } from "./actions";
import { EMPTY_GRANT, type GrantDraft, type GrantProblem, grantOf } from "./grant";
import { GrantFields } from "./grant-fields";

const NO_NAMES: Record<string, string> = {};
const NO_SCOPES: string[] = [];

/** Where the API's MCP endpoint is: a path, never translated. */
const MCP_PATH = "/mcp";

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
  const t = useT<"agents" | "apps">();
  const [isCreating, setIsCreating] = useState(false);
  const notice = settings ? capNotice(t, settings) : null;
  const views = useMemo(
    (): View[] => [
      { label: t("agents.list_view_active"), params: {} },
      { label: t("agents.list_view_with_revoked"), params: { revoked: ["true"] } },
    ],
    [t],
  );

  const columns: TableColumn<Agent>[] = [
    {
      key: "name",
      header: t("agents.agent"),
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
      header: t("common.kind"),
      width: pixel(220),
      renderCell: (a) => (
        <VStack gap={0.5} hAlign="start">
          <Badge label={kindLabel(t, a.kind)} />
          {a.user && (
            <Text type="supporting" color="secondary">
              {t("agents.list_for_person", { person: people[a.user] ?? a.user })}
            </Text>
          )}
        </VStack>
      ),
    },
    {
      key: "state",
      header: t("common.status"),
      width: pixel(120),
      renderCell: (a) => {
        const state = agentState(t, a);
        return <Badge label={state.label} variant={state.variant} />;
      },
    },
    {
      key: "last_used_at",
      header: t("agents.last_used"),
      width: pixel(170),
      renderCell: (a) =>
        a.last_used_at ? (
          <DateTime value={a.last_used_at} short />
        ) : (
          <Text color="secondary">{t("agents.never")}</Text>
        ),
    },
  ];

  return (
    <VStack gap={4}>
      {notice && <Banner status="info" title={notice.title} description={notice.description} />}
      {canWrite && (
        <HStack hAlign="end">
          <Button
            label={t("agents.create_agent")}
            variant="primary"
            onClick={() => setIsCreating(true)}
          />
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
        views={views}
        error={page?.error}
        empty={{
          icon: <CpuChipIcon width={32} />,
          title: t("agents.list_empty_title"),
          description: t("agents.list_empty_description", { path: MCP_PATH }),
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
  const t = useT<"agents">();
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
        title={
          created
            ? t("agents.create_ready_title", { name: name.trim() })
            : t("agents.create_dialog_title")
        }
        subtitle={created ? t("agents.create_ready_subtitle") : t("agents.create_subtitle")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {created ? (
          <>
            <CodeBlock
              code={created.key}
              title={t("agents.create_key_title")}
              isWrapped
              width="100%"
            />
            <Banner
              status="warning"
              title={t("agents.create_key_warning_title")}
              description={t("agents.create_key_warning_description", { path: MCP_PATH })}
            />
          </>
        ) : (
          <>
            {problem && !problem.field && <Banner status="error" title={problem.error} />}
            {problem?.field && problem.field !== "name" && (
              <Banner
                status="error"
                title={t("agents.grant_cannot_save")}
                description={problem.error}
              />
            )}
            <TextInput
              label={t("common.name")}
              description={t("agents.create_name_description")}
              placeholder={t("agents.create_name_placeholder")}
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
              label={t("common.done")}
              variant="primary"
              onClick={() => router.push(`/agents/${created.id}`)}
            />
          ) : (
            <>
              <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
              <Button
                label={t("agents.create_dialog_submit")}
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
