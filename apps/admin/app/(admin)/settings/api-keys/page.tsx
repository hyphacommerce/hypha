import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import type { SearchParams } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { channelNames, requireStaff, sessionClient } from "@/lib/hypha";
import { SETTINGS_CRUMB } from "@/lib/navigation";

import { ApiKeys } from "./api-keys";

/** Settings → API keys: the header renders at once; the keys stream in. */
export default async function ApiKeysPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const params = await searchParams;
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="API keys"
          description="Keys for your own systems (secret) and your storefronts (publishable)."
        />
        <Suspense fallback={<ApiKeys />}>
          <ApiKeyRows params={params} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function ApiKeyRows({ params }: { params: SearchParams }) {
  const client = await sessionClient();
  const [me, result, channelName] = await Promise.all([
    requireStaff(),
    client.GET("/admin/api-keys", { params: { query: pageQuery(params) } }),
    channelNames(client),
  ]);
  return (
    <ApiKeys
      page={listPage(result, "/settings/api-keys", params)}
      channels={Object.fromEntries(channelName)}
      scopes={me.scopes}
      canWrite={me.scopes.includes("api_keys:write")}
    />
  );
}
