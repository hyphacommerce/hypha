"use client";

import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { SaveBar } from "@/components/save-bar";
import type { Agent } from "@/lib/agents";

import { saveAgent } from "../actions";
import {
  type GrantDraft,
  GrantFields,
  type GrantProblem,
  draftOf,
  grantOf,
  operationsOf,
  sameOperations,
  sameScopes,
} from "../grant-fields";

/**
 * An agent's name and grant, one draft saved by the bar at the top. The page keys it by
 * the agent's `updated_at`, so a save starts a fresh draft from what was saved.
 */
export function AgentEditor({
  agent,
  myScopes,
  canWrite,
}: {
  agent: Agent;
  myScopes: readonly string[];
  canWrite: boolean;
}) {
  const saved = draftOf(agent.grant);
  const [name, setName] = useState(agent.name);
  const [grant, setGrant] = useState<GrantDraft>(saved);
  const [problem, setProblem] = useState<GrantProblem>();
  const [isPending, startTransition] = useTransition();

  const nameChanged = name.trim() !== agent.name;
  const scopesChanged = !sameScopes(grant, saved);
  const operationsChanged = !sameOperations(grant, saved);
  const isDirty = nameChanged || scopesChanged || operationsChanged;

  const discard = () => {
    setName(agent.name);
    setGrant(saved);
    setProblem(undefined);
  };

  const save = () => {
    if (!isDirty || !name.trim()) return;
    startTransition(async () => {
      const result = await saveAgent(agent.id, {
        name: nameChanged ? name.trim() : undefined,
        // A grant is checked against what the saver holds: send it only when its scopes
        // changed, so editing the allowlist doesn't need every scope it names.
        grant: scopesChanged ? grantOf(grant) : undefined,
        operations: !scopesChanged && operationsChanged ? operationsOf(grant) : undefined,
      });
      setProblem(
        result.error
          ? { error: result.error, field: result.field, scope: result.scope }
          : undefined,
      );
    });
  };

  return (
    <>
      {canWrite && (
        <SaveBar
          isDirty={isDirty}
          isPending={isPending}
          error={problem?.error}
          canSave={name.trim() !== ""}
          onSave={save}
          onDiscard={discard}
        />
      )}
      <div className="detail-columns">
        <div>
          <GrantFields
            value={grant}
            onChange={setGrant}
            myScopes={myScopes}
            cap={agent.writes_cap}
            problem={problem}
            isReadOnly={!canWrite}
          />
        </div>
        <div>
          <Card>
            <VStack gap={4}>
              <Text type="display-3" as="h2">
                Details
              </Text>
              <TextInput
                label="Name"
                value={name}
                onChange={setName}
                onEnter={save}
                isReadOnly={!canWrite}
                status={
                  !name.trim()
                    ? { type: "error", message: "An agent needs a name." }
                    : problem?.field === "name"
                      ? { type: "error", message: problem.error }
                      : undefined
                }
              />
              <MetadataList>
                {agent.client && (
                  <MetadataListItem label="Client">
                    <VStack gap={0}>
                      <Text>{agent.client.name}</Text>
                      <Text type="supporting" color="secondary">
                        {agent.client.client_id}
                      </Text>
                    </VStack>
                  </MetadataListItem>
                )}
                <MetadataListItem label="Last used">
                  {agent.last_used_at ? (
                    <DateTime value={agent.last_used_at} />
                  ) : (
                    <Text color="secondary">Never</Text>
                  )}
                </MetadataListItem>
                <MetadataListItem label="Created">
                  <DateTime value={agent.created_at} />
                </MetadataListItem>
                {agent.revoked_at && (
                  <MetadataListItem label="Revoked">
                    <DateTime value={agent.revoked_at} />
                  </MetadataListItem>
                )}
              </MetadataList>
            </VStack>
          </Card>
        </div>
      </div>
    </>
  );
}
