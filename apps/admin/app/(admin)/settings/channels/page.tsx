import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { SETTINGS_CRUMB } from "@/lib/navigation";
import { loaded, sessionClient } from "@/lib/hypha";

import { ChannelsCard } from "./channels";

/** Settings → Channels: where products are sold, and the keys storefronts sell through them
 * with. */
export default async function ChannelsSettingsPage() {
  const client = await sessionClient();
  const channels = await client.GET("/admin/channels").then(loaded);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Channels"
          description="Where your products are sold: your online store, AI agents, or anywhere else. A product is sold in the channels it's published to, from its page."
        />
        <ChannelsCard channels={channels.items} />
      </VStack>
    </Section>
  );
}
