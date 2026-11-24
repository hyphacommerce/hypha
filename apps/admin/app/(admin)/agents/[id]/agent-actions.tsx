"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Switch } from "@astryxdesign/core/Switch";
import { useState, useTransition } from "react";

import { useT } from "@/lib/use-t";

import { revokeAgent, setAgentEnabled } from "../actions";

/**
 * Turning an agent off and on (at once: its credentials stop until it's on again), and
 * revoking it for good, asked first.
 */
export function AgentActions({
  id,
  name,
  isEnabled,
}: {
  id: string;
  name: string;
  isEnabled: boolean;
}) {
  const t = useT<"agents">();
  const [isConfirming, setIsConfirming] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const revoke = () =>
    startTransition(async () => {
      const result = await revokeAgent(id);
      setIsConfirming(false);
      setError(result.error);
    });

  return (
    <VStack gap={2} hAlign="end">
      <HStack gap={3} vAlign="center">
        <Switch
          label={t("common.enabled")}
          value={isEnabled}
          changeAction={async (enabled) => setError((await setAgentEnabled(id, enabled)).error)}
        />
        <Button
          label={t("common.revoke")}
          variant="destructive"
          onClick={() => setIsConfirming(true)}
        />
      </HStack>
      {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
      <AlertDialog
        isOpen={isConfirming}
        onOpenChange={setIsConfirming}
        title={t("agents.revoke_dialog_title", { name })}
        description={t("agents.revoke_dialog_description")}
        actionLabel={t("agents.revoke_dialog_action")}
        isActionLoading={isPending}
        onAction={revoke}
      />
    </VStack>
  );
}
