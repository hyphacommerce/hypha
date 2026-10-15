import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";

import { PageHeader } from "@/components/page-header";
import { formatBytes, formatDateTime } from "@/lib/format";
import { loaded, sessionClient } from "@/lib/hypha";

import { AssetForm } from "./asset-form";
import { DeleteAsset } from "./delete-asset";

type Asset = AdminSchemas["AssetResponse"];

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
      return <video src={asset.url} controls style={{ maxWidth: "100%", maxHeight: 520 }} />;
    case "file":
      return (
        <Link href={asset.url} isExternalLink>
          Open {asset.filename}
        </Link>
      );
  }
}

export default async function AssetPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const client = await sessionClient();
  const asset = loaded(await client.GET("/admin/assets/{id}", { params: { path: { id } } }));

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title={asset.title ?? asset.filename}
          back={{ label: "Media", href: "/media" }}
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
              <MetadataListItem label="Uploaded">{formatDateTime(asset.created_at)}</MetadataListItem>
            </MetadataList>
          </VStack>
        </HStack>
      </VStack>
    </Section>
  );
}
