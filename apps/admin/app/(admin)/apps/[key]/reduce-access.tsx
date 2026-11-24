"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxList, CheckboxListItem } from "@astryxdesign/core/CheckboxList";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState, useTransition } from "react";

import { isPersonalDataScope, personalDataLabel } from "@/lib/apps";
import { useT } from "@/lib/use-t";

import { narrowGrant } from "../actions";

type Scope = AdminSchemas["Scope"];

/**
 * Taking scopes away from an installed app's grant: the ones unticked stop working at
 * once, for tokens it holds already too. Adding one back takes a new authorization from
 * the app, which staff approve.
 */
export function ReduceAccess({
  appKey,
  name,
  scopes,
}: {
  appKey: string;
  name: string;
  scopes: Scope[];
}) {
  const t = useT<"apps">();
  const [isOpen, setIsOpen] = useState(false);
  const [kept, setKept] = useState<string[]>(scopes);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const removed = scopes.filter((scope) => !kept.includes(scope));

  const open = () => {
    setKept(scopes);
    setError(undefined);
    setIsOpen(true);
  };
  const save = () =>
    startTransition(async () => {
      const result = await narrowGrant(
        appKey,
        scopes.filter((scope) => kept.includes(scope)),
      );
      if (result.error) setError(result.error);
      else setIsOpen(false);
    });

  return (
    <>
      <Button label={t("apps.reduce_button")} variant="secondary" size="sm" onClick={open} />
      {isOpen && (
        <Dialog isOpen onOpenChange={setIsOpen} width={520} purpose="form">
          <DialogHeader
            title={t("apps.reduce_dialog_title", { name })}
            subtitle={t("apps.reduce_dialog_subtitle")}
            onOpenChange={setIsOpen}
          />
          <VStack gap={4} padding={4}>
            {error && <Banner status="error" title={error} />}
            <CheckboxList label={t("apps.reduce_kept")} value={kept} onChange={setKept}>
              {scopes.map((scope) => (
                <CheckboxListItem
                  key={scope}
                  value={scope}
                  label={scope}
                  description={isPersonalDataScope(scope) ? personalDataLabel(t, scope) : undefined}
                />
              ))}
            </CheckboxList>
            <Text type="supporting" color="secondary">
              {t("apps.reduce_hint")}
            </Text>
            <HStack gap={2} hAlign="end">
              <Button label={t("common.cancel")} variant="ghost" onClick={() => setIsOpen(false)} />
              <Button
                label={t("apps.reduce_remove", { count: removed.length })}
                variant="destructive"
                isDisabled={removed.length === 0}
                isLoading={isPending}
                onClick={save}
              />
            </HStack>
          </VStack>
        </Dialog>
      )}
    </>
  );
}
