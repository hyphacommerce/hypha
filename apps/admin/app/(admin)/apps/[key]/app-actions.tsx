"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { useState, useTransition } from "react";

import { useT } from "@/lib/use-t";

import { removeApp, setAppEnabled } from "../actions";

/** Turning an app off or on, and uninstalling or removing it, each asked first. */
export function AppActions({
  appKey,
  name,
  isEnabled,
  isInstalled,
  isUninstalled,
}: {
  appKey: string;
  name: string;
  isEnabled: boolean;
  isInstalled: boolean;
  isUninstalled: boolean;
}) {
  const t = useT<"apps">();
  const [isConfirming, setIsConfirming] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  // An installed app is uninstalled first (it hears app.uninstalled); after that, or for
  // one made by hand, the same call removes it for good.
  const isUninstall = isInstalled && !isUninstalled;

  const toggle = () =>
    startTransition(async () => setError((await setAppEnabled(appKey, !isEnabled)).error));
  const remove = () =>
    startTransition(async () => {
      const result = await removeApp(appKey, !isUninstall);
      setIsConfirming(false);
      setError(result.error);
    });

  return (
    <VStack gap={2} hAlign="end">
      <HStack gap={2}>
        {!isUninstalled && (
          <Button
            label={isEnabled ? t("common.turn_off") : t("common.turn_on")}
            variant="secondary"
            isLoading={isPending && !isConfirming}
            onClick={toggle}
          />
        )}
        <Button
          label={isUninstall ? t("apps.actions_uninstall") : t("common.remove")}
          variant="destructive"
          onClick={() => setIsConfirming(true)}
        />
      </HStack>
      {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
      <AlertDialog
        isOpen={isConfirming}
        onOpenChange={setIsConfirming}
        title={
          isUninstall
            ? t("apps.actions_uninstall_title", { name })
            : t("apps.actions_remove_title", { name })
        }
        description={
          isUninstall
            ? t("apps.actions_uninstall_description", { event: "app.uninstalled" })
            : t("apps.actions_remove_description")
        }
        actionLabel={isUninstall ? t("apps.actions_uninstall") : t("common.remove")}
        isActionLoading={isPending}
        onAction={remove}
      />
    </VStack>
  );
}
