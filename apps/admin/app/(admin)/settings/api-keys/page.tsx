import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { SETTINGS_CRUMB } from "@/lib/navigation";
import type { SearchParams } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { channelNames, requireStaff, sessionClient } from "@/lib/hypha";

import { ApiKeys } from "./api-keys";

export default async function ApiKeysPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const params = await searchParams;
  const client = await sessionClient();
  const [me, result, channelName] = await Promise.all([
    requireStaff(),
    client.GET("/admin/api-keys", { params: { query: pageQuery(params) } }),
    channelNames(client),
  ]);
  const page = listPage(result, "/settings/api-keys", params);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="API keys"
          description="Keys for your own systems (secret) and your storefronts (publishable)."
        />
        <ApiKeys
          page={page}
          channels={Object.fromEntries(channelName)}
          scopes={me.scopes}
          canWrite={me.scopes.includes("api_keys:write")}
        />
      </VStack>
    </Section>
  );
}
