"use client";

// Deleting a customer: only one without orders; with orders the button isn't shown and
// erasure takes its place.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Button } from "@astryxdesign/core/Button";
import { TrashIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { deleteCustomer } from "../actions";

export function DeleteCustomer({
  id,
  name,
  canDelete,
}: {
  id: string;
  /** What to call the customer in the dialog. */
  name: string;
  /** The API's `can_delete`: false once the customer has any order. */
  canDelete: boolean;
}) {
  const router = useRouter();
  const [isOpen, setIsOpen] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  // With orders there's nothing to delete: erasure (step 7) takes the button's place.
  if (!canDelete) return null;
  return (
    <>
      <Button label="Delete" variant="ghost" icon={<TrashIcon />} onClick={() => setIsOpen(true)} />
      {isOpen && (
        <AlertDialog
          isOpen
          onOpenChange={(open) => !open && setIsOpen(false)}
          title={`Delete ${name}?`}
          description={
            error ??
            "The customer, their addresses and custom field values go for good. Their open carts stay, without a customer."
          }
          actionLabel="Delete customer"
          isActionLoading={isPending}
          onAction={() =>
            startTransition(async () => {
              const result = await deleteCustomer(id);
              if (result.error) {
                setError(result.error);
                return;
              }
              router.push("/customers");
              router.refresh();
            })
          }
        />
      )}
    </>
  );
}
