"use client";

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Text } from "@astryxdesign/core/Text";
import { useEffect, useState, useTransition } from "react";

import { type SearchStatus, indexStatus, rebuildIndex } from "./actions";

/**
 * Whether search is up to date: what the index holds and what's waiting to be rebuilt.
 * While something waits, it looks again every 2 seconds (the sync job's pace), so a
 * rebuild shows its progress.
 */
export function IndexStatus({ status: initial }: { status: SearchStatus }) {
  const [status, setStatus] = useState(initial);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const isBusy = status.queued > 0;

  useEffect(() => {
    if (!isBusy) return;
    // Each look waits for the last answer, so a slow one never overlaps the next; an
    // answer that arrives after the page stopped waiting is dropped.
    let isWaiting = true;
    let timer: ReturnType<typeof setTimeout>;
    const look = async () => {
      const result = await indexStatus();
      if (!isWaiting) return;
      if (result.status) setStatus(result.status);
      setError(result.error);
      timer = setTimeout(() => void look(), 2000);
    };
    timer = setTimeout(() => void look(), 2000);
    return () => {
      isWaiting = false;
      clearTimeout(timer);
    };
  }, [isBusy]);

  const rebuild = () =>
    startTransition(async () => {
      const result = await rebuildIndex();
      if (result.status) setStatus(result.status);
      setError(result.error);
    });

  return (
    <Card>
      <VStack gap={4}>
        <HStack gap={2} hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            Search index
          </Text>
          {isBusy ? (
            <Badge label="Updating" variant="warning" />
          ) : (
            <Badge label="Up to date" variant="success" />
          )}
        </HStack>
        {error && <Banner status="error" title={error} />}
        <Text color="secondary">
          {isBusy
            ? `${status.queued.toLocaleString()} ${status.queued === 1 ? "product is" : "products are"} waiting to be rebuilt. Search keeps answering meanwhile, with the old data for those.`
            : "Every product change has reached search. Edits to products, prices and stock show within a few seconds."}
        </Text>
        <MetadataList>
          <MetadataListItem label="Searchable results">
            <Text>{status.documents.toLocaleString()}</Text>
          </MetadataListItem>
          <MetadataListItem label="Waiting">
            <Text>{status.queued.toLocaleString()}</Text>
          </MetadataListItem>
        </MetadataList>
        <Text type="supporting" color="secondary">
          The index holds one entry per variant in each market that sells it. Rebuild it only after
          restoring a database, or if search shows something it shouldn&apos;t.
        </Text>
        <HStack hAlign="end">
          <Button label="Rebuild index" isLoading={isPending} onClick={rebuild} />
        </HStack>
      </VStack>
    </Card>
  );
}
