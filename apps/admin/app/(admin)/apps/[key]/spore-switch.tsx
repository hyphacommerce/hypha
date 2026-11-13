"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { VStack } from "@astryxdesign/core/Layout";
import { Switch } from "@astryxdesign/core/Switch";
import { useState } from "react";

import { useT } from "@/lib/use-t";

import { setAppEnabled } from "../actions";

/**
 * Turning a built-in spore on or off: all staff may change about it. Off, its payment
 * providers, event handlers, routes and blocks stop.
 */
export function SporeSwitch({ appKey, isEnabled }: { appKey: string; isEnabled: boolean }) {
  const t = useT();
  const [error, setError] = useState<string>();
  return (
    <VStack gap={2} hAlign="end">
      <Switch
        // The setting, not one of its states: "On" stayed "On" when it was off.
        label={t("common.enabled")}
        value={isEnabled}
        changeAction={async (enabled) => setError((await setAppEnabled(appKey, enabled)).error)}
      />
      {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
    </VStack>
  );
}
