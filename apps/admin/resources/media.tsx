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
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
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
import { labelOr } from "@/lib/keys";
import type { KeyIn } from "@/lib/messages";
import { type Format, useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

type Asset = AdminSchemas["AssetResponse"];

const SORT_OPTIONS: { value: string; label: KeyIn<"common"> }[] = [
  { value: "newest", label: "common.sort_newest" },
  { value: "oldest", label: "common.sort_oldest" },
  { value: "name_asc", label: "common.sort_name_asc" },
  { value: "name_desc", label: "common.sort_name_desc" },
];

const SORTS: Record<string, SortDef> = {
  filename: { asc: "name_asc", desc: "name_desc" },
  uploaded: { asc: "oldest", desc: "newest" },
};

const VIEWS: { label: KeyIn<"media" | "common">; params: View["params"] }[] = [
  { label: "common.all", params: {} },
  { label: "media.view_images", params: { kind: ["image"] } },
  { label: "media.view_videos", params: { kind: ["video"] } },
  { label: "media.view_files", params: { kind: ["file"] } },
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

function facts(asset: Asset, format: Format): string {
  return asset.width && asset.height
    ? `${asset.width} × ${asset.height} · ${format.bytes(asset.byte_size)}`
    : format.bytes(asset.byte_size);
}

function MediaGrid({ assets }: { assets: Asset[] }) {
  const format = useFormat();
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
              <Text type="supporting">{facts(asset, format)}</Text>
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
  const t = useT<"media">();
  return (
    <SegmentedControl
      label={t("media.layout_label")}
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
      <SegmentedControlItem value="grid" label={t("media.layout_grid")} />
      <SegmentedControlItem value="table" label={t("media.layout_table")} />
    </SegmentedControl>
  );
}

export function MediaLibrary({
  page,
  view,
}: {
  /** The page of rows; absent while it loads (the table shows its toolbar meanwhile). */
  page?: { items: Asset[]; next_cursor: string | null; error?: string };
  view: "grid" | "table";
}) {
  const t = useT<"media">();
  const format = useFormat();
  // Kinds are views, so the only filter is the search; `kind=` in a URL still picks its
  // view.
  const filters: FilterDef[] = [
    {
      kind: "search",
      param: "q",
      label: t("common.search"),
      placeholder: t("media.search_placeholder"),
    },
  ];
  const views: View[] = VIEWS.map(({ label, params }) => ({ label: t(label), params }));
  const sortOptions: SortOption[] = SORT_OPTIONS.map(({ value, label }) => ({
    value,
    label: t(label),
  }));
  const columns: TableColumn<Asset>[] = [
    {
      key: "filename",
      header: t("media.file"),
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
    {
      key: "kind",
      header: t("common.kind"),
      width: pixel(90),
      renderCell: (asset) => {
        // The API may add kinds: one without a name shows as it is.
        const key = `media.kind_${asset.kind}`;
        return labelOr(t, key, asset.kind);
      },
    },
    { key: "facts", header: t("media.size"), renderCell: (asset) => facts(asset, format) },
    {
      key: "alt",
      header: t("media.alt_text"),
      renderCell: (asset) => asset.alt ?? <Text color="secondary">{t("common.none")}</Text>,
    },
    {
      key: "uploaded",
      header: t("media.uploaded"),
      width: pixel(190),
      renderCell: (asset) => <DateTime value={asset.created_at} short />,
    },
  ];
  return (
    <ResourceTable<Asset>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="id"
      rowHref={(a) => `/media/${a.id}`}
      filters={filters}
      views={views}
      sorts={SORTS}
      sortOptions={sortOptions}
      defaultSort="newest"
      error={page?.error}
      empty={{
        icon: <PhotoIcon width={32} />,
        title: t("media.empty_title"),
        description: t("media.empty_description"),
      }}
      noResults={{ title: t("media.no_results_title") }}
      toolbarEnd={<ViewToggle view={view} />}
      renderRows={view === "grid" ? (assets) => <MediaGrid assets={assets} /> : undefined}
    />
  );
}
