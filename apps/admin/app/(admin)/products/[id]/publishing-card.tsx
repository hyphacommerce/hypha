"use client";

import { Card } from "@astryxdesign/core/Card";
import { CheckboxList, CheckboxListItem } from "@astryxdesign/core/CheckboxList";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { useT } from "@/lib/use-t";

import type { Draft } from "./draft";

/**
 * The channels a product is sold in: part of the editor's draft, saved with the bar like
 * its categories.
 */
export function PublishingCard({
  channels,
  draft,
  isActive,
  onChange,
}: {
  channels: AdminSchemas["ChannelResponse"][];
  draft: Draft;
  isActive: boolean;
  onChange: (change: Partial<Draft>) => void;
}) {
  const t = useT<"products">();
  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("products.publishing_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {isActive
              ? t("products.publishing_hint_active")
              : t("products.publishing_hint_inactive")}{" "}
            <Link href="/settings/channels">{t("products.publishing_manage")}</Link>
          </Text>
        </VStack>
        <CheckboxList
          label={t("products.publishing_channels_label")}
          isLabelHidden
          value={draft.channels}
          onChange={(next) => onChange({ channels: next.toSorted() })}
        >
          {channels.map((channel) => (
            <CheckboxListItem key={channel.id} value={channel.id} label={channel.name} />
          ))}
        </CheckboxList>
      </VStack>
    </Card>
  );
}
