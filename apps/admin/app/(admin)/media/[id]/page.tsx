import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { Suspense } from "react";

import { DateTime } from "@/components/date-time";
import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import { formatBytes } from "@/lib/format";
import { loaded, sessionClient } from "@/lib/hypha";

import { AssetForm } from "./asset-form";
import { DeleteAsset } from "./delete-asset";

type Asset = AdminSchemas["AssetResponse"];

const BACK: Crumb = { label: "Media", href: "/media" };

/** The file itself, as large as the column allows. */
function FilePreview({ asset }: { asset: Asset }) {
  if (!asset.url) return <Text color="secondary">The file isn&apos;t ready.</Text>;
  switch (asset.kind) {
    case "image":
      return (
        <img
          src={asset.url}
          alt={asset.alt ?? ""}
          style={{ maxWidth: "100%", maxHeight: 520, objectFit: "contain" }}
        />
      );
    case "video":
      // A preview of the merchant's own upload: there are no caption files to offer.
      // oxlint-disable-next-line jsx-a11y/media-has-caption
      return <video src={asset.url} controls style={{ maxWidth: "100%", maxHeight: 520 }} />;
    case "file":
      return (
        <Link href={asset.url} isExternalLink>
          Open {asset.filename}
        </Link>
      );
  }
}

/**
 * A file: its preview beside its form, what uses it and its details. The way back
 * renders at once; the file streams in behind a skeleton of the page.
 */
export default async function AssetPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <Suspense fallback={<DetailSkeleton back={BACK} />}>
          <Asset id={id} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Asset({ id }: { id: string }) {
  const client = await sessionClient();
  const [assetResult, usagesResult] = await Promise.all([
    client.GET("/admin/assets/{id}", { params: { path: { id } } }),
    client.GET("/admin/assets/{id}/usages", { params: { path: { id }, query: { limit: 25 } } }),
  ]);
  const asset = loaded(assetResult);
  const usages = loaded(usagesResult);
  // Products show by title; other owners (spores) by kind and ID.
  const titles = new Map(
    await Promise.all(
      usages.items
        .filter((u) => u.owner.kind === "product")
        .map(async (u) => {
          const { data } = await client.GET("/admin/products/{id}", {
            params: { path: { id: u.owner.id } },
          });
          return [u.owner.id, data?.title ?? u.owner.id] as const;
        }),
    ),
  );

  return (
    <>
      <PageHeader
        title={asset.title ?? asset.filename}
        back={BACK}
        actions={<DeleteAsset id={asset.id} />}
      />
      <HStack gap={5} vAlign="start" wrap="wrap">
        <Card padding={4}>
          <FilePreview asset={asset} />
        </Card>
        <VStack gap={5} width={360}>
          <Card padding={4}>
            <AssetForm
              id={asset.id}
              alt={asset.alt ?? ""}
              title={asset.title ?? ""}
              hasAlt={asset.kind === "image"}
            />
          </Card>
          <Card padding={4}>
            <VStack gap={2}>
              <Text type="label">Used by</Text>
              {usages.items.length === 0 ? (
                <Text type="supporting" color="secondary">
                  Nothing yet. Files in use can&apos;t be deleted.
                </Text>
              ) : (
                <>
                  {usages.items.map((u) =>
                    u.owner.kind === "product" ? (
                      <Link key={u.owner.id} href={`/products/${u.owner.id}`}>
                        {titles.get(u.owner.id)}
                        {u.role === "featured" ? " (featured)" : ""}
                      </Link>
                    ) : (
                      <Text key={`${u.owner.kind}:${u.owner.id}`}>
                        {u.owner.kind} {u.owner.id}
                      </Text>
                    ),
                  )}
                  {usages.has_more && (
                    <Text type="supporting" color="secondary">
                      And more.
                    </Text>
                  )}
                </>
              )}
            </VStack>
          </Card>
          <MetadataList title="Details">
            <MetadataListItem label="File">{asset.filename}</MetadataListItem>
            <MetadataListItem label="Type">{asset.mime_type}</MetadataListItem>
            <MetadataListItem label="Size">{formatBytes(asset.byte_size)}</MetadataListItem>
            {asset.width && asset.height ? (
              <MetadataListItem label="Dimensions">
                {asset.width} × {asset.height} px
              </MetadataListItem>
            ) : null}
            <MetadataListItem label="Visibility">
              {asset.visibility === "public" ? "Public" : "Private"}
            </MetadataListItem>
            <MetadataListItem label="Uploaded">
              <DateTime value={asset.created_at} />
            </MetadataListItem>
          </MetadataList>
        </VStack>
      </HStack>
    </>
  );
}
