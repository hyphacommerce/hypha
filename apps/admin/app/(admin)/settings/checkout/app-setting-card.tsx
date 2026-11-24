"use client";

// An app checkout settings name: the one verifying shipping addresses
// (`address.verify`: asked when a cart's address is set and when staff change an
// order's; it only warns and suggests, a checkout rule on the verdict makes it block), or
// the one suggesting addresses as people type (`address.suggest` and `address.resolve`).

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { useT } from "@/lib/use-t";

import { type AppSetting, setCheckoutApp } from "./actions";

/** "None" in the picker. */
const NONE = "none";

/** What each setting's card says: `checkout.app_<part>_title`, `…_description`, … */
const PART = { verification_app: "verification", autocomplete_app: "autocomplete" } as const;

export function AppSettingCard({
  setting,
  current,
  apps,
  canWrite,
}: {
  setting: AppSetting;
  /** The app named now, if any. */
  current: string | null;
  /** The installed apps that answer the setting's points: key and name. */
  apps: { value: string; label: string }[];
  canWrite: boolean;
}) {
  const t = useT<"checkout">();
  const router = useRouter();
  const [chosen, setChosen] = useState(current ?? NONE);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  // The app named stays a choice even when it no longer answers, so it can be seen.
  const part = PART[setting];
  const options = [
    { value: NONE, label: t(`checkout.app_${part}_none`) },
    ...apps,
    ...(current && !apps.some((a) => a.value === current)
      ? [{ value: current, label: t("checkout.app_no_longer_answers", { app: current }) }]
      : []),
  ];
  const save = () =>
    startTransition(async () => {
      const result = await setCheckoutApp(setting, chosen === NONE ? null : chosen);
      setError(result.error);
      if (!result.error) router.refresh();
    });

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t(`checkout.app_${part}_title`)}
          </Text>
          <Text color="secondary">{t(`checkout.app_${part}_description`)}</Text>
        </VStack>
        {error && <Banner status="error" title={error} />}
        {apps.length === 0 && !current ? (
          <Text color="secondary">{t(`checkout.app_${part}_empty`)}</Text>
        ) : (
          <HStack gap={2} vAlign="end" wrap="wrap">
            <Selector
              label={t(`checkout.app_${part}_picker`)}
              options={options}
              value={chosen}
              onChange={setChosen}
              isDisabled={!canWrite}
            />
            {canWrite && (
              <Button
                label={t("common.save")}
                variant="primary"
                isLoading={isPending}
                isDisabled={chosen === (current ?? NONE)}
                onClick={save}
              />
            )}
          </HStack>
        )}
      </VStack>
    </Card>
  );
}
