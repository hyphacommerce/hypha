"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState, useTransition } from "react";

import { deleteProduct, setProductStatus } from "../actions";

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

export function StatusActions({
  id,
  title,
  status,
}: {
  id: string;
  title: string;
  status: Status;
}) {
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState<Status>();
  const [confirming, setConfirming] = useState(false);
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
        <Button
          label="Delete"
          variant="secondary"
          isDisabled={pending !== undefined}
          onClick={() => setConfirming(true)}
        />
      </HStack>
      {error && <Banner status="error" title={error} />}
      {confirming && <DeleteDialog id={id} title={title} onClose={() => setConfirming(false)} />}
    </VStack>
  );
}

function DeleteDialog({ id, title, onClose }: { id: string; title: string; onClose: () => void }) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={`Delete ${title}?`}
      description={
        error ??
        "It's removed with its variants, prices and stock records, and leaves every sales channel. Its media stay in the library and orders keep their lines. Archive it instead to keep it."
      }
      actionLabel="Delete product"
      isActionLoading={isPending}
      onAction={() =>
        startTransition(async () => {
          // On success the action redirects to the list.
          const result = await deleteProduct(id);
          if (result.error) setError(result.error);
        })
      }
    />
  );
}
