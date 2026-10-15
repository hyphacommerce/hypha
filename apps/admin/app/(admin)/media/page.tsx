import { AspectRatio } from "@astryxdesign/core/AspectRatio";
import { Button } from "@astryxdesign/core/Button";
import { Center } from "@astryxdesign/core/Center";
import { ClickableCard } from "@astryxdesign/core/ClickableCard";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Grid } from "@astryxdesign/core/Grid";
import { Icon } from "@astryxdesign/core/Icon";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { DocumentIcon, FilmIcon, PhotoIcon } from "@heroicons/react/24/outline";
import { redirect } from "next/navigation";

import { PageHeader } from "@/components/page-header";
import { Pager } from "@/components/pager";
import { formatBytes } from "@/lib/format";
import { loaded, sessionClient } from "@/lib/hypha";

import { Uploader } from "./uploader";

type Kind = AdminSchemas["AssetKindDto"];
type Asset = AdminSchemas["AssetResponse"];

const FILTERS: { label: string; kind?: Kind }[] = [
  { label: "All" },
  { label: "Images", kind: "image" },
  { label: "Videos", kind: "video" },
  { label: "Files", kind: "file" },
];

const KIND_ICONS = { image: PhotoIcon, video: FilmIcon, file: DocumentIcon };

function asKind(value: string | string[] | undefined): Kind | undefined {
  return FILTERS.find((f) => f.kind === value)?.kind;
}

/** A square preview: the thumbnail for images, the kind's icon for the rest. */
function Preview({ asset }: { asset: Asset }) {
  return (
    <AspectRatio ratio={1} fit={asset.thumbnail_url ? "cover" : undefined}>
      {asset.thumbnail_url ? (
        // A plain <img>: the media URLs resize, so no Next.js image pipeline (0011 §2).
        <img src={asset.thumbnail_url} alt={asset.alt ?? ""} loading="lazy" />
      ) : (
        <Center>
          <Icon icon={KIND_ICONS[asset.kind]} size="lg" color="secondary" />
        </Center>
      )}
    </AspectRatio>
  );
}

export default async function MediaPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const params = await searchParams;
  const kind = asKind(params.kind);
  const cursor = typeof params.cursor === "string" ? params.cursor : undefined;

  const client = await sessionClient();
  const result = await client.GET("/admin/assets", { params: { query: { kind, cursor } } });
  // A hand-edited URL (an old or foreign cursor): start the list again, same filter.
  if (result.response.status === 400) redirect(kind ? `/media?kind=${kind}` : "/media");
  const page = loaded(result);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader title="Media" />
        <Uploader />

        <HStack gap={1}>
          {FILTERS.map((filter) => (
            <Button
              key={filter.label}
              label={filter.label}
              size="sm"
              variant={filter.kind === kind ? "secondary" : "ghost"}
              href={filter.kind ? `/media?kind=${filter.kind}` : "/media"}
            />
          ))}
        </HStack>

        {page.items.length === 0 ? (
          <EmptyState
            icon={<PhotoIcon width={32} />}
            title={cursor ? "No more files" : kind ? `No ${kind}s yet` : "No media yet"}
            description="Files you upload show up here, newest first, ready to use on products."
          />
        ) : (
          <Grid columns={{ minWidth: 180 }} gap={3}>
            {page.items.map((asset) => (
              <ClickableCard
                key={asset.id}
                label={asset.title ?? asset.filename}
                href={`/media/${asset.id}`}
                padding={0}
              >
                <VStack gap={0}>
                  <Preview asset={asset} />
                  <VStack gap={0.5} padding={3}>
                    <Text weight="medium" maxLines={1}>
                      {asset.title ?? asset.filename}
                    </Text>
                    <Text type="supporting">
                      {asset.width && asset.height
                        ? `${asset.width} × ${asset.height} · ${formatBytes(asset.byte_size)}`
                        : formatBytes(asset.byte_size)}
                    </Text>
                  </VStack>
                </VStack>
              </ClickableCard>
            ))}
          </Grid>
        )}

        <Pager path="/media" query={{ kind }} isFirstPage={!cursor} nextCursor={page.next_cursor} />
      </VStack>
    </Section>
  );
}
