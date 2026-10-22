"use client";

// Answering a client's connection: a name and the grant, then Connect or Cancel. Either
// answer is a URL of the client's (its redirect URI with a code, or `access_denied`):
// the browser goes there, leaving the admin.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import { type ConnectResult, approveConnection, denyConnection } from "../actions";
import { type GrantDraft, type GrantProblem, grantOf } from "../grant";
import { GrantFields } from "../grant-fields";

export function ConnectForm({
  id,
  clientName,
  redirectHost,
  grant: suggested,
  requested,
  cap,
  myScopes,
}: {
  id: string;
  clientName: string;
  redirectHost: string;
  /** The grant to start from: the API's suggestion. */
  grant: GrantDraft;
  /** The Hypha scopes the client asked for, as a hint. */
  requested: string[];
  /** The deployment's cap on what agents write. */
  cap: string;
  myScopes: readonly string[];
}) {
  const [name, setName] = useState(clientName);
  const [grant, setGrant] = useState<GrantDraft>(suggested);
  const [problem, setProblem] = useState<GrantProblem>();
  const [leaving, setLeaving] = useState(false);
  const [isPending, startTransition] = useTransition();
  const [answering, setAnswering] = useState<"connect" | "cancel">();

  const answer = (kind: "connect" | "cancel", run: () => Promise<ConnectResult>) => {
    setAnswering(kind);
    startTransition(async () => {
      const result = await run();
      if (result.redirectTo) {
        setLeaving(true);
        // The client's own URL: a full navigation, not the admin's router.
        window.location.assign(result.redirectTo);
      } else {
        setProblem(
          result.error
            ? { error: result.error, field: result.field, scope: result.scope }
            : undefined,
        );
      }
    });
  };

  if (leaving) {
    return <Text color="secondary">Taking you back to {redirectHost}…</Text>;
  }

  return (
    <VStack gap={5}>
      <Card>
        <VStack gap={3}>
          <TextInput
            label="Name"
            description="How the agent shows in the admin and the proposals inbox, so say where it runs."
            value={name}
            onChange={setName}
            status={
              !name.trim()
                ? { type: "error", message: "The agent needs a name." }
                : problem?.field === "name"
                  ? { type: "error", message: problem.error }
                  : undefined
            }
          />
          {requested.length > 0 && (
            <Text type="supporting" color="secondary">
              It asked for {requested.join(", ")}. That's only a hint: you choose below.
            </Text>
          )}
          <Text type="supporting" color="secondary">
            It never does more than your own role allows, and it stops if your account is
            deactivated. Writes are proposals you approve unless you choose Apply.
          </Text>
        </VStack>
      </Card>

      <GrantFields
        value={grant}
        onChange={setGrant}
        myScopes={myScopes}
        cap={cap}
        problem={problem}
      />

      <Card>
        <VStack gap={3}>
          {problem && <Banner status="error" title={problem.error} />}
          <Text color="secondary">
            Connecting sends {clientName} back to {redirectHost} with access to this store as set
            above. You can change or revoke it any time under Agents.
          </Text>
          <HStack gap={2}>
            <Button
              label="Connect"
              variant="primary"
              isLoading={isPending && answering === "connect"}
              isDisabled={!name.trim() || isPending}
              onClick={() =>
                answer("connect", () => approveConnection(id, name.trim(), grantOf(grant)))
              }
            />
            <Button
              label="Cancel"
              variant="secondary"
              isLoading={isPending && answering === "cancel"}
              isDisabled={isPending}
              onClick={() => answer("cancel", () => denyConnection(id))}
            />
          </HStack>
        </VStack>
      </Card>
    </VStack>
  );
}
