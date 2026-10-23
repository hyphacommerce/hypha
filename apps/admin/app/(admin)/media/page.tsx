import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one, oneOf } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { sessionClient } from "@/lib/hypha";
import { MediaLibrary } from "@/resources/media";

import { Uploader } from "./uploader";

const KINDS: AdminSchemas["AssetKindDto"][] = ["image", "video", "file"];
const SORTS = ["newest", "oldest", "name_asc", "name_desc"] as const;

/** The header, the uploader and the library's toolbar render at once; the files stream in. */
export default async function MediaPage({ searchParams }: { searchParams: Promise<SearchParams> }) {
  const params = await searchParams;
  const view = one(params, "view") === "table" ? "table" : "grid";
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader title="Media" />
        <Uploader />
        <Suspense fallback={<MediaLibrary view={view} />}>
          <MediaRows params={params} view={view} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function MediaRows({ params, view }: { params: SearchParams; view: "grid" | "table" }) {
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
  return <MediaLibrary page={listPage(result, "/media", params)} view={view} />;
}
