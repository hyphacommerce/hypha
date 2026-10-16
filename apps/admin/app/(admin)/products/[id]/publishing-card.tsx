"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxList, CheckboxListItem } from "@astryxdesign/core/CheckboxList";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useState } from "react";

import { setProductChannels } from "../actions";

/** The channels a product is sold in (design note 0020): each tick saves at once. */
export function PublishingCard({
  id,
  published,
  channels,
  isActive,
}: {
  id: string;
  published: string[];
  channels: AdminSchemas["ChannelResponse"][];
  isActive: boolean;
}) {
  const [value, setValue] = useState(published);
  const [error, setError] = useState<string>();

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
        {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
        <CheckboxList
          label="Sales channels"
          isLabelHidden
          value={value}
          changeAction={async (next) => {
            const result = await setProductChannels(id, next);
            setError(result.error);
            if (!result.error) setValue(next);
          }}
        >
          {channels.map((channel) => (
            <CheckboxListItem key={channel.id} value={channel.id} label={channel.name} />
          ))}
        </CheckboxList>
      </VStack>
    </Card>
  );
}
