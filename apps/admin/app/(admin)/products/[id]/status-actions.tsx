"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useState, useTransition } from "react";

import { setProductStatus } from "../actions";

type Status = AdminSchemas["ProductStatus"];

/**
 * The moves a product's status allows (the domain's rule in `Product::set_status`):
 * a draft goes live, a live product goes back to draft or is archived, and an archived
 * one comes back as a draft.
 */
const MOVES: Record<Status, { to: Status; label: string; primary?: boolean }[]> = {
  draft: [{ to: "active", label: "Publish", primary: true }],
  active: [
    { to: "draft", label: "Unpublish" },
    { to: "archived", label: "Archive" },
  ],
  archived: [{ to: "draft", label: "Restore as draft" }],
};

export function StatusActions({ id, status }: { id: string; status: Status }) {
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState<Status>();
  const [, startTransition] = useTransition();

  const move = (to: Status) => {
    setPending(to);
    startTransition(async () => {
      const result = await setProductStatus(id, to);
      setError(result.error);
      setPending(undefined);
    });
  };

  return (
    <VStack gap={2}>
      <HStack gap={2}>
        {MOVES[status].map((m) => (
          <Button
            key={m.to}
            label={m.label}
            variant={m.primary ? "primary" : "secondary"}
            isLoading={pending === m.to}
            isDisabled={pending !== undefined && pending !== m.to}
            onClick={() => move(m.to)}
          />
        ))}
      </HStack>
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}
