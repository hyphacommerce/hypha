"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Switch } from "@astryxdesign/core/Switch";
import { useState, useTransition } from "react";

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
          label="Enabled"
          value={isEnabled}
          changeAction={async (enabled) => setError((await setAgentEnabled(id, enabled)).error)}
        />
        <Button label="Revoke" variant="destructive" onClick={() => setIsConfirming(true)} />
      </HStack>
      {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
      <AlertDialog
        isOpen={isConfirming}
        onOpenChange={setIsConfirming}
        title={`Revoke ${name}?`}
        description="Its credentials stop at once and for good, and its pending proposals can't be approved any more. To pause it instead, turn it off. This can't be undone."
        actionLabel="Revoke agent"
        isActionLoading={isPending}
        onAction={revoke}
      />
    </VStack>
  );
}
