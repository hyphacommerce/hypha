"use client";

// Erasing a customer's personal data: a request with a cancel window, run by a job;
// and the export answering a data request.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { ArrowDownTrayIcon, ShieldExclamationIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { useT } from "@/lib/use-t";

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
  const t = useT<"customers">();
  const [isOpen, setIsOpen] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  if (!canErase || isErased || erasureDueAt) return null;
  return (
    <>
      <Button
        label={t("customers.erase_button")}
        variant="ghost"
        icon={<ShieldExclamationIcon />}
        onClick={() => setIsOpen(true)}
      />
      {isOpen && (
        <AlertDialog
          isOpen
          onOpenChange={(open) => !open && setIsOpen(false)}
          title={t("customers.erase_dialog_title", { name })}
          description={error ?? t("customers.erase_dialog_description")}
          actionLabel={t("customers.erase_dialog_action")}
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
  const t = useT<"customers">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  if (erasedAt) {
    return (
      <Banner
        status="info"
        title={t("customers.erased_banner_title")}
        description={
          <Text>
            {t.rich("customers.erased_banner_description", {
              time: <DateTime key="time" value={erasedAt} />,
            })}
          </Text>
        }
      />
    );
  }
  if (!erasureDueAt) return null;
  return (
    <Banner
      status="warning"
      title={t("customers.erasure_banner_title")}
      description={
        <VStack gap={2}>
          <Text>
            {t.rich("customers.erasure_banner_description", {
              time: <DateTime key="time" value={erasureDueAt} />,
            })}
          </Text>
          {error && <Text type="supporting">{error}</Text>}
          {canErase && (
            <HStack>
              <Button
                label={t("customers.erasure_cancel")}
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

/** Downloads the customer's data as one JSON document. */
export function ExportCustomer({ id }: { id: string }) {
  const t = useT<"customers">();
  return (
    <Button
      label={t("customers.export_button")}
      variant="ghost"
      icon={<ArrowDownTrayIcon />}
      href={`/customers/${id}/export`}
    />
  );
}
