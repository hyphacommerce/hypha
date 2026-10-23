"use client";

// Picks an image from the media library: a category's image and the emails' logo. Only
// ready, public images can be picked: both are shown to people who aren't signed in.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { SelectableCard } from "@astryxdesign/core/SelectableCard";
import { Spinner } from "@astryxdesign/core/Spinner";
import { Text } from "@astryxdesign/core/Text";
import { PhotoIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useEffect, useEffectEvent, useState, useTransition } from "react";

import { libraryPage } from "@/app/(admin)/products/actions";
import { LibrarySearch } from "@/components/library-search";

type Asset = AdminSchemas["AssetResponse"];

/** The library's images, newest first; pick one. */
export function ImagePicker({
  isSaving,
  onPick,
  onClose,
}: {
  isSaving: boolean;
  onPick: (asset: string) => void;
  onClose: () => void;
}) {
  const [assets, setAssets] = useState<Asset[]>();
  const [next, setNext] = useState<string>();
  const [error, setError] = useState<string>();
  const [picked, setPicked] = useState<string>();
  const [isLoading, startLoading] = useTransition();

  const [query, setQuery] = useState("");
  const load = (cursor?: string, q = query) =>
    startLoading(async () => {
      const page = await libraryPage(cursor, q);
      if ("error" in page) {
        setError(page.error);
        return;
      }
      // A category shows an image: videos aren't offered.
      const images = page.items.filter((a) => a.kind === "image");
      setAssets((before) => [...(cursor ? (before ?? []) : []), ...images]);
      setNext(page.next);
    });
  // The first page, once, when the dialog opens.
  const loadFirst = useEffectEvent(() => load());
  useEffect(() => loadFirst(), []);

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={760} purpose="form">
      <DialogHeader
        title="Pick an image"
        subtitle="Images from the library, newest first."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
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
              title={query ? `No images match “${query}”` : "No images yet"}
              description={
                query
                  ? "Search looks at file names, alt text and titles."
                  : "Upload images on the Media page."
              }
            />
          ) : (
            <Grid columns={{ minWidth: 130 }} gap={2}>
              {assets.map((asset) => (
                <SelectableCard
                  key={asset.id}
                  label={asset.title ?? asset.filename}
                  isSelected={picked === asset.id}
                  isDisabled={asset.status !== "ready" || asset.visibility !== "public"}
                  onChange={(isSelected) => setPicked(isSelected ? asset.id : undefined)}
                  padding={0}
                >
                  <VStack gap={0}>
                    <img
                      src={asset.thumbnail_url ?? asset.url ?? ""}
                      alt=""
                      style={{ width: "100%", aspectRatio: "1", objectFit: "cover" }}
                    />
                    <VStack gap={0} padding={2}>
                      <Text type="supporting" maxLines={1}>
                        {asset.title ?? asset.filename}
                      </Text>
                    </VStack>
                  </VStack>
                </SelectableCard>
              ))}
            </Grid>
          )}
          {next && (
            <HStack hAlign="center">
              <Button
                label="Load more"
                variant="ghost"
                isLoading={isLoading}
                onClick={() => load(next)}
              />
            </HStack>
          )}
        </VStack>
      </div>
      <VStack gap={3} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Use image"
            variant="primary"
            isDisabled={!picked}
            isLoading={isSaving}
            onClick={() => picked && onPick(picked)}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
