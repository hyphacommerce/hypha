import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { loaded, sessionClient } from "@/lib/hypha";
import { getStoreT } from "@/lib/get-store-t";
import { getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";

import { ChannelsCard } from "./channels";

/** Settings → Channels: where products are sold, and the keys storefronts sell through them
 * with. The header renders at once; the channels stream in. */
export default async function ChannelsSettingsPage() {
  const t = await getT();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={settingsCrumb(t)}
          title={t("settings.channels_title")}
          description={t("settings.channels_description")}
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
  const [channels, storeT] = await Promise.all([
    client.GET("/admin/channels").then(loaded),
    getStoreT(),
  ]);
  const keyNames = Object.fromEntries(
    channels.items.map((channel) => [
      channel.id,
      storeT("defaults.channel_key_name", { channel: channel.name }),
    ]),
  );
  return <ChannelsCard channels={channels.items} keyNames={keyNames} />;
}
