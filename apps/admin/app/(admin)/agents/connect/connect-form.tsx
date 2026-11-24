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

import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

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
  const t = useT<"agents">();
  const format = useFormat();
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
    return <Text color="secondary">{t("agents.connect_leaving", { host: redirectHost })}</Text>;
  }

  return (
    <VStack gap={5}>
      <Card>
        <VStack gap={3}>
          <TextInput
            label={t("common.name")}
            description={t("agents.connect_name_description")}
            value={name}
            onChange={setName}
            status={
              !name.trim()
                ? { type: "error", message: t("agents.connect_name_required") }
                : problem?.field === "name"
                  ? { type: "error", message: problem.error }
                  : undefined
            }
          />
          {requested.length > 0 && (
            <Text type="supporting" color="secondary">
              {t("agents.connect_requested", { scopes: format.list(requested, "unit") })}
            </Text>
          )}
          <Text type="supporting" color="secondary">
            {t("agents.connect_limits")}
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
            {t("agents.connect_summary", { client: clientName, host: redirectHost })}
          </Text>
          <HStack gap={2}>
            <Button
              label={t("agents.connect_button")}
              variant="primary"
              isLoading={isPending && answering === "connect"}
              isDisabled={!name.trim() || isPending}
              onClick={() =>
                answer("connect", () => approveConnection(id, name.trim(), grantOf(grant)))
              }
            />
            <Button
              label={t("common.cancel")}
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
