"use client";

import { Card } from "@astryxdesign/core/Card";
import { CheckboxList, CheckboxListItem } from "@astryxdesign/core/CheckboxList";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";

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
  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Publishing
          </Text>
          <Text type="supporting" color="secondary">
            {isActive
              ? "The channels that sell this product."
              : "The channels that sell this product once it's active: drafts and archived products aren't sold anywhere."}{" "}
            <Link href="/settings/channels">Manage channels</Link>
          </Text>
        </VStack>
        <CheckboxList
          label="Sales channels"
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
