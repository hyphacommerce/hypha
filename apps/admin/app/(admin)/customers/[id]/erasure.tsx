"use client";

// Erasing a customer's personal data (design note 0032, §7.3): a request with a cancel
// window, run by a job; and the export answering a data request (§7.4).

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { ArrowDownTrayIcon, ShieldExclamationIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";

import { cancelErasure, requestErasure } from "../actions";

/** The header's erase button: asks first, since the window is the only way back. */
export function EraseCustomer({
  id,
  name,
  canErase,
  erasureDueAt,
  isErased,
}: {
  id: string;
  name: string;
  /** `customers:erase`. */
  canErase: boolean;
  /** When the erasure runs, while one is requested. */
  erasureDueAt: string | null | undefined;
  isErased: boolean;
}) {
  const router = useRouter();
  const [isOpen, setIsOpen] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  if (!canErase || isErased || erasureDueAt) return null;
  return (
    <>
      <Button
        label="Erase personal data"
        variant="ghost"
        icon={<ShieldExclamationIcon />}
        onClick={() => setIsOpen(true)}
      />
      {isOpen && (
        <AlertDialog
          isOpen
          onOpenChange={(open) => !open && setIsOpen(false)}
          title={`Erase ${name}'s personal data?`}
          description={
            error ??
            "After the cancel window, their name, contact details, addresses, notes and consent are removed from the customer and their orders, carts and emails. Orders, totals and countries stay. Apps are told to delete their copies. This can't be undone once it runs."
          }
          actionLabel="Request erasure"
          isActionLoading={isPending}
          onAction={() =>
            startTransition(async () => {
              const result = await requestErasure(id);
              if (result.error) {
                setError(result.error);
                return;
              }
              router.refresh();
              setIsOpen(false);
            })
          }
        />
      )}
    </>
  );
}

/** What the page shows about an erasure: pending with its countdown, or done. */
export function ErasureBanner({
  id,
  erasureDueAt,
  erasedAt,
  canErase,
}: {
  id: string;
  erasureDueAt: string | null | undefined;
  erasedAt: string | null | undefined;
  canErase: boolean;
}) {
  const router = useRouter();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  if (erasedAt) {
    return (
      <Banner
        status="info"
        title="This customer's personal data was erased"
        description={
          <Text>
            On <DateTime value={erasedAt} />. What stays: their orders' statistics, their groups and
            a placeholder email that can't receive mail. The customer is read-only.
          </Text>
        }
      />
    );
  }
  if (!erasureDueAt) return null;
  return (
    <Banner
      status="warning"
      title="Erasure requested"
      description={
        <VStack gap={2}>
          <Text>
            Their personal data is removed on <DateTime value={erasureDueAt} />. Until then it can
            be cancelled.
          </Text>
          {error && <Text type="supporting">{error}</Text>}
          {canErase && (
            <HStack>
              <Button
                label="Cancel erasure"
                variant="secondary"
                size="sm"
                isLoading={isPending}
                onClick={() =>
                  startTransition(async () => {
                    const result = await cancelErasure(id);
                    if (result.error) setError(result.error);
                    else router.refresh();
                  })
                }
              />
            </HStack>
          )}
        </VStack>
      }
    />
  );
}

/** Downloads the customer's data as one JSON document (design note 0032, §7.4). */
export function ExportCustomer({ id }: { id: string }) {
  return (
    <Button
      label="Export data"
      variant="ghost"
      icon={<ArrowDownTrayIcon />}
      href={`/customers/${id}/export`}
    />
  );
}
