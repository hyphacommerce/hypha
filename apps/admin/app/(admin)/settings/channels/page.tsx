import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { loaded, sessionClient } from "@/lib/hypha";
import { SETTINGS_CRUMB } from "@/lib/navigation";

import { ChannelsCard } from "./channels";

/** Settings → Channels: where products are sold, and the keys storefronts sell through them
 * with. The header renders at once; the channels stream in. */
export default function ChannelsSettingsPage() {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Channels"
          description="Where your products are sold: your online store, AI agents, or anywhere else. A product is sold in the channels it's published to, from its page."
        />
        <Suspense fallback={<CardSkeleton lines={4} />}>
          <Channels />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Channels() {
  const client = await sessionClient();
  const channels = await client.GET("/admin/channels").then(loaded);
  return <ChannelsCard channels={channels.items} />;
}
