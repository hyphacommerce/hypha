"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Text } from "@astryxdesign/core/Text";
import { useEffect, useState, useTransition } from "react";

import { type SearchStatus, indexStatus, rebuildIndex } from "./actions";

/**
 * What the index holds and what's waiting to be rebuilt. While something waits, it
 * looks again every 2 seconds (the sync job's pace), so a rebuild shows its progress.
 */
export function IndexStatus({ status: initial }: { status: SearchStatus }) {
  const [status, setStatus] = useState(initial);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  useEffect(() => {
    if (status.queued === 0) return;
    const timer = setTimeout(async () => {
      const result = await indexStatus();
      if (result.status) setStatus(result.status);
      setError(result.error);
    }, 2000);
    return () => clearTimeout(timer);
  }, [status]);

  const rebuild = () =>
    startTransition(async () => {
      const result = await rebuildIndex();
      if (result.status) setStatus(result.status);
      setError(result.error);
    });

  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          Search index
        </Text>
        {error && <Banner status="error" title={error} />}
        <MetadataList>
          <MetadataListItem label="Documents">
            <Text>{status.documents.toLocaleString()}</Text>
          </MetadataListItem>
          <MetadataListItem label="Waiting to be rebuilt">
            <Text>{status.queued === 0 ? "Up to date" : status.queued.toLocaleString()}</Text>
          </MetadataListItem>
        </MetadataList>
        <Text type="supporting" color="secondary">
          One document per variant in each market that sells it. Changes reach search within a
          few seconds; rebuild everything only after restoring a database or if search looks
          wrong.
        </Text>
        <HStack hAlign="end">
          <Button label="Rebuild index" isLoading={isPending} onClick={rebuild} />
        </HStack>
      </VStack>
    </Card>
  );
}
