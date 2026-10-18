"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxList, CheckboxListItem } from "@astryxdesign/core/CheckboxList";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useState, useTransition } from "react";

import { isPersonalDataScope, personalDataLabel } from "@/lib/apps";

import { narrowGrant } from "../actions";

type Scope = AdminSchemas["Scope"];

/**
 * Taking scopes away from an installed app's grant (design note 0029, §4.1): the ones
 * unticked stop working at once, for tokens it holds already too. Adding one back takes
 * a new authorization from the app, which staff approve.
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
      <Button label="Reduce access" variant="secondary" size="sm" onClick={open} />
      {isOpen && (
        <Dialog isOpen onOpenChange={setIsOpen} width={520} purpose="form">
          <DialogHeader
            title={`Reduce ${name}'s access`}
            subtitle="Untick what the app should no longer do. It loses those scopes at once and is told its grant changed."
            onOpenChange={setIsOpen}
          />
          <VStack gap={4} padding={4}>
            {error && <Banner status="error" title={error} />}
            <CheckboxList label="Scopes the app keeps" value={kept} onChange={setKept}>
              {scopes.map((scope) => (
                <CheckboxListItem
                  key={scope}
                  value={scope}
                  label={scope}
                  description={isPersonalDataScope(scope) ? personalDataLabel(scope) : undefined}
                />
              ))}
            </CheckboxList>
            <Text type="supporting" color="secondary">
              To give a scope back, the app asks again and staff approve it.
            </Text>
            <HStack gap={2} hAlign="end">
              <Button label="Cancel" variant="ghost" onClick={() => setIsOpen(false)} />
              <Button
                label={
                  removed.length === 0
                    ? "Remove scopes"
                    : `Remove ${removed.length} ${removed.length === 1 ? "scope" : "scopes"}`
                }
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
