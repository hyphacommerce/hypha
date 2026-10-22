"use client";

// The media library: a grid of previews, or a table, on the same URL-driven list as
// every other.

import { AspectRatio } from "@astryxdesign/core/AspectRatio";
import { ClickableCard } from "@astryxdesign/core/ClickableCard";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { SegmentedControl, SegmentedControlItem } from "@astryxdesign/core/SegmentedControl";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { DocumentIcon, FilmIcon, PhotoIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { href, withChanges } from "@/components/resource-table/params";
import {
  type FilterDef,
  ResourceTable,
  type SortDef,
  type SortOption,
  type View,
} from "@/components/resource-table/resource-table";
import { formatBytes } from "@/lib/format";

type Asset = AdminSchemas["AssetResponse"];

const SORT_OPTIONS: SortOption[] = [
  { value: "newest", label: "Newest" },
  { value: "oldest", label: "Oldest" },
  { value: "name_asc", label: "Name A–Z" },
  { value: "name_desc", label: "Name Z–A" },
];

const SORTS: Record<string, SortDef> = {
  filename: { asc: "name_asc", desc: "name_desc" },
  uploaded: { asc: "oldest", desc: "newest" },
};

// Kinds are views (below), so the only filter is the search; `kind=` in a URL still
// picks its view.
const FILTERS: FilterDef[] = [
  { kind: "search", param: "q", label: "Search", placeholder: "Search names, alt text, titles" },
];

const VIEWS: View[] = [
  { label: "All", params: {} },
  { label: "Images", params: { kind: ["image"] } },
  { label: "Videos", params: { kind: ["video"] } },
  { label: "Files", params: { kind: ["file"] } },
];

// Keyed by string: the API may add kinds; those show as files.
const KIND_ICONS: Record<string, typeof DocumentIcon | undefined> = {
  image: PhotoIcon,
  video: FilmIcon,
  file: DocumentIcon,
};

/** A preview: the thumbnail for images, the kind's icon for the rest. */
function Preview({ asset, size }: { asset: Asset; size?: number }) {
  const KindIcon = KIND_ICONS[asset.kind] ?? DocumentIcon;
  const body = asset.thumbnail_url ? (
    // A plain <img>: the media URLs resize, so no Next.js image pipeline.
    <img src={asset.thumbnail_url} alt={asset.alt ?? ""} loading="lazy" />
  ) : (
    <Text color="secondary">
      <KindIcon width={size ? size / 2 : 40} aria-hidden />
    </Text>
  );
  if (!size) {
    return (
      <AspectRatio ratio={1} fit={asset.thumbnail_url ? "cover" : "center"}>
        {body}
      </AspectRatio>
    );
  }
  return (
    <div style={{ width: size, height: size, borderRadius: 6, overflow: "hidden", flexShrink: 0 }}>
      <AspectRatio ratio={1} fit={asset.thumbnail_url ? "cover" : "center"}>
        {body}
      </AspectRatio>
    </div>
  );
}

function facts(asset: Asset): string {
  return asset.width && asset.height
    ? `${asset.width} × ${asset.height} · ${formatBytes(asset.byte_size)}`
    : formatBytes(asset.byte_size);
}

function MediaGrid({ assets }: { assets: Asset[] }) {
  return (
    <Grid columns={{ minWidth: 180 }} gap={3}>
      {assets.map((asset) => (
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
              <Text type="supporting">{facts(asset)}</Text>
            </VStack>
          </VStack>
        </ClickableCard>
      ))}
    </Grid>
  );
}

/** Grid or table, kept in the URL (`view=table`) so the page survives a reload. */
function ViewToggle({ view }: { view: "grid" | "table" }) {
  const router = useRouter();
  const pathname = usePathname();
  const params = useSearchParams();
  const [, startTransition] = useTransition();
  return (
    <SegmentedControl
      label="View"
      size="sm"
      value={view}
      onChange={(next) =>
        startTransition(() =>
          router.replace(
            href(pathname, withChanges(params, { view: next === "table" ? next : undefined })),
            {
              scroll: false,
            },
          ),
        )
      }
    >
      <SegmentedControlItem value="grid" label="Grid" />
      <SegmentedControlItem value="table" label="Table" />
    </SegmentedControl>
  );
}

export function MediaLibrary({
  page,
  view,
}: {
  page: { items: Asset[]; next_cursor: string | null; error?: string };
  view: "grid" | "table";
}) {
  const columns: TableColumn<Asset>[] = [
    {
      key: "filename",
      header: "File",
      width: proportional(3, { minWidth: 240 }),
      renderCell: (asset) => (
        <HStack gap={3} vAlign="center">
          <Preview asset={asset} size={40} />
          <VStack gap={0.5}>
            <Link href={`/media/${asset.id}`} weight="semibold" color="primary">
              {asset.title ?? asset.filename}
            </Link>
            {asset.title && (
              <Text type="supporting" color="secondary">
                {asset.filename}
              </Text>
            )}
          </VStack>
        </HStack>
      ),
    },
    { key: "kind", header: "Kind", width: pixel(90) },
    { key: "facts", header: "Size", renderCell: facts },
    {
      key: "alt",
      header: "Alt text",
      renderCell: (asset) => asset.alt ?? <Text color="secondary">None</Text>,
    },
    {
      key: "uploaded",
      header: "Uploaded",
      width: pixel(190),
      renderCell: (asset) => <DateTime value={asset.created_at} short />,
    },
  ];
  return (
    <ResourceTable<Asset>
      rows={page.items}
      nextCursor={page.next_cursor}
      columns={columns}
      idKey="id"
      rowHref={(a) => `/media/${a.id}`}
      filters={FILTERS}
      views={VIEWS}
      sorts={SORTS}
      sortOptions={SORT_OPTIONS}
      defaultSort="newest"
      error={page.error}
      empty={{
        icon: <PhotoIcon width={32} />,
        title: "No media yet",
        description: "Files you upload show up here, newest first, ready to use on products.",
      }}
      noResults={{ title: "No files match these filters" }}
      toolbarEnd={<ViewToggle view={view} />}
      renderRows={view === "grid" ? (assets) => <MediaGrid assets={assets} /> : undefined}
    />
  );
}
