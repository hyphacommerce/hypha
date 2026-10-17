"use client";

import { AspectRatio } from "@astryxdesign/core/AspectRatio";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { FileInput } from "@astryxdesign/core/FileInput";
import { Grid } from "@astryxdesign/core/Grid";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { SelectableCard } from "@astryxdesign/core/SelectableCard";
import { Spinner } from "@astryxdesign/core/Spinner";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";
import {
  ArrowLeftIcon,
  ArrowRightIcon,
  FilmIcon,
  PhotoIcon,
  XMarkIcon,
} from "@heroicons/react/24/outline";
import { type DragEvent, useEffect, useRef, useState, useTransition } from "react";

import { ACCEPT_PRODUCT_MEDIA, MAX_BYTES, uploadFile } from "../../media/upload";
import { LibrarySearch } from "@/components/library-search";
import { libraryPage, setProductMedia } from "../actions";

type Item = AdminSchemas["ProductMediaResponse"];
type Asset = AdminSchemas["AssetResponse"];

/** Shopify's limit, and the API's (design note 0013, section 5). */
const MAX_ITEMS = 250;

/** A square preview: the thumbnail of an image, the first frame of a video. */
function Preview({ item }: { item: Pick<Item, "kind" | "url" | "thumbnail_url" | "alt"> }) {
  return (
    <AspectRatio ratio={1} fit="cover">
      {item.kind === "image" ? (
        <img src={item.thumbnail_url ?? item.url} alt={item.alt ?? ""} draggable={false} />
      ) : item.kind === "video" && item.url ? (
        <video src={item.url} muted preload="metadata" />
      ) : (
        <Text color="secondary">
          <PhotoIcon width={32} aria-hidden />
        </Text>
      )}
    </AspectRatio>
  );
}

/** `list` with the item at `from` moved to `to`. */
function moved<T>(list: T[], from: number, to: number): T[] {
  const next = [...list];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item);
  return next;
}

/**
 * The product's media list (design note 0014, section 8): drag to reorder, or use the
 * arrows; the first item is featured. Every change saves the whole list at once.
 * Removing an item detaches it; the file stays in the library.
 */
export function MediaSection({ id, media }: { id: string; media: Item[] }) {
  const [items, setItems] = useState(media);
  // The server's list after each save, taken in while staying mounted: a remount would
  // drop the errors and running uploads of this section.
  const [stored, setStored] = useState(media);
  if (media !== stored) {
    setStored(media);
    setItems(media);
  }
  // The list as last rendered, for an upload finishing after other changes were saved:
  // it appends to that, not to the list it started with.
  const latest = useRef(items);
  useEffect(() => {
    latest.current = items;
  });
  const [dragged, setDragged] = useState<number>();
  const [error, setError] = useState<string>();
  const [isSaving, startSaving] = useTransition();
  const [uploads, setUploads] = useState<File[]>([]);
  const [isUploading, setIsUploading] = useState(false);
  const [isPicking, setIsPicking] = useState(false);

  const save = (next: Item[]) => {
    setItems(next);
    setError(undefined);
    startSaving(async () => {
      const result = await setProductMedia(
        id,
        next.map((item) => item.asset_id),
      );
      if (result.error) {
        setError(result.error);
        setItems(media);
      }
    });
  };

  /** Appends assets to the current list; a message if that failed. */
  const attach = async (assets: string[]): Promise<string | undefined> => {
    const ids = [...latest.current.map((item) => item.asset_id), ...assets];
    return (await setProductMedia(id, ids)).error;
  };

  const uploadAll = async (selected: File | File[] | null) => {
    const files = selected === null ? [] : Array.isArray(selected) ? selected : [selected];
    if (files.length === 0) return;
    setIsUploading(true);
    setError(undefined);
    const uploaded: string[] = [];
    const failed: string[] = [];
    // One at a time, like the Media page.
    for (const file of files) {
      const result = await uploadFile(file);
      if ("error" in result) failed.push(`${file.name}: ${result.error}`);
      else uploaded.push(result.id);
    }
    if (uploaded.length > 0) {
      const error = await attach(uploaded);
      if (error) failed.push(error);
    }
    if (failed.length > 0) setError(failed.join(" · "));
    setUploads([]);
    setIsUploading(false);
  };

  const onDrop = (to: number) => (e: DragEvent) => {
    e.preventDefault();
    if (dragged !== undefined && dragged !== to) save(moved(items, dragged, to));
    setDragged(undefined);
  };

  return (
    <Card>
      <VStack gap={4}>
        <HStack gap={2} vAlign="center" hAlign="between" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Media
            </Text>
            <Text type="supporting" color="secondary">
              Drag to reorder. The first is the featured image.
            </Text>
          </VStack>
          <HStack gap={2} vAlign="center">
            {isSaving && <Spinner size="sm" aria-label="Saving" />}
            <Button
              label="Pick from library"
              variant="secondary"
              isDisabled={items.length >= MAX_ITEMS}
              onClick={() => setIsPicking(true)}
            />
          </HStack>
        </HStack>

        {error && <Banner status="error" title={error} />}

        {items.length === 0 ? (
          <EmptyState
            icon={<PhotoIcon width={32} />}
            title="No media yet"
            description="Upload images or videos, or pick them from the library."
          />
        ) : (
          <Grid columns={{ minWidth: 150 }} gap={3}>
            {items.map((item, index) => (
              <div
                key={item.asset_id}
                draggable
                onDragStart={(e) => {
                  setDragged(index);
                  e.dataTransfer.effectAllowed = "move";
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  e.dataTransfer.dropEffect = "move";
                }}
                onDrop={onDrop(index)}
                onDragEnd={() => setDragged(undefined)}
                data-testid="product-media-item"
                style={{ cursor: "grab", opacity: dragged === index ? 0.4 : 1 }}
              >
                <Card padding={0}>
                  <VStack gap={0}>
                    <Preview item={item} />
                    <HStack gap={1} padding={2} vAlign="center" hAlign="between">
                      {index === 0 ? (
                        <Badge label="Featured" variant="info" />
                      ) : (
                        <Text color="secondary">
                          {item.kind === "video" ? (
                            <FilmIcon width={16} aria-label="Video" />
                          ) : (
                            <PhotoIcon width={16} aria-label="Image" />
                          )}
                        </Text>
                      )}
                      <HStack gap={0.5}>
                        <IconButton
                          label="Move earlier"
                          tooltip="Move earlier"
                          size="sm"
                          variant="ghost"
                          icon={<ArrowLeftIcon width={16} />}
                          isDisabled={index === 0}
                          onClick={() => save(moved(items, index, index - 1))}
                        />
                        <IconButton
                          label="Move later"
                          tooltip="Move later"
                          size="sm"
                          variant="ghost"
                          icon={<ArrowRightIcon width={16} />}
                          isDisabled={index === items.length - 1}
                          onClick={() => save(moved(items, index, index + 1))}
                        />
                        <IconButton
                          label="Remove from product"
                          tooltip="Remove from product (the file stays in the library)"
                          size="sm"
                          variant="ghost"
                          icon={<XMarkIcon width={16} />}
                          onClick={() => save(items.filter((_, i) => i !== index))}
                        />
                      </HStack>
                    </HStack>
                  </VStack>
                </Card>
              </div>
            ))}
          </Grid>
        )}

        <FileInput
          label="Upload media"
          isLabelHidden
          mode="dropzone"
          isMultiple
          accept={ACCEPT_PRODUCT_MEDIA}
          maxSize={MAX_BYTES}
          value={uploads}
          onChange={(selected) => setUploads(Array.isArray(selected) ? selected : [])}
          changeAction={uploadAll}
          isLoading={isUploading}
          isDisabled={items.length >= MAX_ITEMS}
          placeholder="Drop images or videos here, or choose files"
        />
      </VStack>

      {isPicking && (
        <LibraryPicker
          attached={new Set(items.map((item) => item.asset_id))}
          room={MAX_ITEMS - items.length}
          onClose={() => setIsPicking(false)}
          onPick={async (assets) => {
            setError(await attach(assets));
            setIsPicking(false);
          }}
        />
      )}
    </Card>
  );
}

/** The library's images and videos; pick several, added in the order picked. */
function LibraryPicker({
  attached,
  room,
  onClose,
  onPick,
}: {
  attached: Set<string>;
  room: number;
  onClose: () => void;
  onPick: (assets: string[]) => Promise<void>;
}) {
  const [assets, setAssets] = useState<Asset[]>();
  const [next, setNext] = useState<string>();
  const [error, setError] = useState<string>();
  const [picked, setPicked] = useState<string[]>([]);
  const [isLoading, startLoading] = useTransition();
  const [isAdding, startAdding] = useTransition();

  const [query, setQuery] = useState("");
  const load = (cursor?: string, q = query) =>
    startLoading(async () => {
      const page = await libraryPage(cursor, q);
      if ("error" in page) {
        setError(page.error);
        return;
      }
      setAssets((before) => [...(cursor ? (before ?? []) : []), ...page.items]);
      setNext(page.next);
    });
  // The first page, once, when the dialog opens.
  useEffect(() => load(), []);

  const toggle = (asset: string, isSelected: boolean) =>
    setPicked((before) =>
      isSelected ? [...before, asset] : before.filter((id) => id !== asset),
    );

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={760} purpose="form">
      <DialogHeader
        title="Pick from library"
        subtitle="Images and videos, newest first."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      {/* The library scrolls; the buttons stay in view (see the shipping option dialog). */}
      <VStack padding={4} gap={0}>
        <LibrarySearch
          onSearch={(q) => {
            setQuery(q);
            load(undefined, q);
          }}
        />
      </VStack>
      <div style={{ flex: "1 1 auto", minHeight: 0, overflowY: "auto" }}>
      <VStack gap={4} padding={4}>
        {assets === undefined ? (
          <HStack hAlign="center" padding={6}>
            <Spinner aria-label="Loading the library" />
          </HStack>
        ) : assets.length === 0 ? (
          <EmptyState
            icon={<PhotoIcon width={32} />}
            title={query ? `Nothing matches “${query}”` : "The library is empty"}
            description={
              query
                ? "Search looks at file names, alt text and titles."
                : "Upload files on the Media page, or drop them on this product."
            }
          />
        ) : (
          <Grid columns={{ minWidth: 130 }} gap={2}>
            {assets.map((asset) => {
              const isAttached = attached.has(asset.id);
              const isPicked = picked.includes(asset.id);
              return (
                <SelectableCard
                  key={asset.id}
                  label={asset.title ?? asset.filename}
                  isSelected={isPicked}
                  isDisabled={isAttached || (!isPicked && picked.length >= room)}
                  onChange={(isSelected) => toggle(asset.id, isSelected)}
                  padding={0}
                >
                  <VStack gap={0}>
                    <Preview
                      item={{
                        kind: asset.kind,
                        url: asset.url ?? "",
                        thumbnail_url: asset.thumbnail_url,
                        alt: asset.alt,
                      }}
                    />
                    <VStack gap={0} padding={2}>
                      <Text type="supporting" maxLines={1}>
                        {asset.title ?? asset.filename}
                      </Text>
                      {isAttached && (
                        <Text type="supporting" color="secondary">
                          On this product
                        </Text>
                      )}
                    </VStack>
                  </VStack>
                </SelectableCard>
              );
            })}
          </Grid>
        )}
        {next && (
          <HStack hAlign="center">
            <Button label="Load more" variant="ghost" isLoading={isLoading} onClick={() => load(next)} />
          </HStack>
        )}
      </VStack>
      </div>
      <VStack gap={3} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={picked.length > 0 ? `Add ${picked.length}` : "Add"}
            variant="primary"
            isDisabled={picked.length === 0}
            isLoading={isAdding}
            onClick={() => startAdding(() => onPick(picked))}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
