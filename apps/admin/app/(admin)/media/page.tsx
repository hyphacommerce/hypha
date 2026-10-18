import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import type { AdminSchemas } from "@hypha/sdk/admin";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one, oneOf } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { sessionClient } from "@/lib/hypha";
import { MediaLibrary } from "@/resources/media";

import { Uploader } from "./uploader";

const KINDS: AdminSchemas["AssetKindDto"][] = ["image", "video", "file"];
const SORTS = ["newest", "oldest", "name_asc", "name_desc"] as const;

export default async function MediaPage({ searchParams }: { searchParams: Promise<SearchParams> }) {
  const params = await searchParams;
  const client = await sessionClient();
  const result = await client.GET("/admin/assets", {
    params: {
      query: {
        ...pageQuery(params),
        q: one(params, "q"),
        kind: oneOf(params, "kind", KINDS),
        sort: oneOf(params, "sort", SORTS),
      },
    },
  });
  const page = listPage(result, "/media", params);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader title="Media" />
        <Uploader />
        <MediaLibrary page={page} view={one(params, "view") === "table" ? "table" : "grid"} />
      </VStack>
    </Section>
  );
}
