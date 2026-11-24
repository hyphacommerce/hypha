"use client";

// The customer's storefront account: whether they have one, where they're signed in, and
// signing them out everywhere.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Text } from "@astryxdesign/core/Text";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { DateTime } from "@/components/date-time";
import { useT } from "@/lib/use-t";

import { revokeSessions } from "../actions";

type Facts = AdminSchemas["AccountFactsResponse"];

export function AccountCard({
  customerId,
  account,
  canEdit,
}: {
  customerId: string;
  /** `null` while the customer never signed in. */
  account: Facts | null | undefined;
  canEdit: boolean;
}) {
  const router = useRouter();
  const t = useT<"customers">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          {t("customers.account_title")}
        </Text>
        {!account ? (
          <Text color="secondary">{t("customers.account_never")}</Text>
        ) : (
          <>
            <MetadataList>
              <MetadataListItem label={t("customers.account_signed_in_on")}>
                {t("customers.account_devices", { count: account.active_sessions })}
              </MetadataListItem>
              {account.last_active_at && (
                <MetadataListItem label={t("customers.account_last_active")}>
                  <DateTime value={account.last_active_at} />
                </MetadataListItem>
              )}
            </MetadataList>
            {error && <Banner status="error" title={error} />}
            {canEdit && account.active_sessions > 0 && (
              <div>
                <Button
                  label={t("customers.account_sign_out_everywhere")}
                  variant="secondary"
                  size="sm"
                  isLoading={isPending}
                  onClick={() =>
                    startTransition(async () => {
                      const result = await revokeSessions(customerId);
                      setError(result.error);
                      if (!result.error) router.refresh();
                    })
                  }
                />
              </div>
            )}
          </>
        )}
      </VStack>
    </Card>
  );
}
