"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState, useTransition } from "react";

import type { KeyIn } from "@/lib/messages";
import { useT } from "@/lib/use-t";

import { deleteProduct, setProductStatus } from "../actions";

type Status = AdminSchemas["ProductStatus"];

/**
 * The moves a product's status allows (the domain's rule in `Product::set_status`):
 * a draft goes live, a live product goes back to draft or is archived, and an archived
 * one comes back as a draft.
 */
const MOVES: Record<
  Status,
  { to: Status; label: KeyIn<"products" | "common">; primary?: boolean }[]
> = {
  draft: [{ to: "active", label: "products.status_publish", primary: true }],
  active: [
    { to: "draft", label: "products.status_unpublish" },
    { to: "archived", label: "common.archive" },
  ],
  archived: [{ to: "draft", label: "products.status_restore" }],
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
  const t = useT<"products">();
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
            label={t(m.label)}
            variant={m.primary ? "primary" : "secondary"}
            isLoading={pending === m.to}
            isDisabled={pending !== undefined && pending !== m.to}
            onClick={() => move(m.to)}
          />
        ))}
        <Button
          label={t("common.delete")}
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
  const t = useT<"products">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={t("common.delete_named", { name: title })}
      description={error ?? t("products.delete_description")}
      actionLabel={t("products.delete_action")}
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
