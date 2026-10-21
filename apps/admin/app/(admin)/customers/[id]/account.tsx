"use client";

// The customer's storefront account (design note 0034, §7): whether they have one, where
// they're signed in, and signing them out everywhere.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Text } from "@astryxdesign/core/Text";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import type { AdminSchemas } from "@hypha/sdk/admin";

import { DateTime } from "@/components/date-time";

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
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          Account
        </Text>
        {!account ? (
          <Text color="secondary">
            Never signed in to the storefront. Checkouts as a guest still count as this customer.
          </Text>
        ) : (
          <>
            <MetadataList>
              <MetadataListItem label="Signed in on">
                {account.active_sessions === 0
                  ? "No devices"
                  : `${account.active_sessions} ${account.active_sessions === 1 ? "device" : "devices"}`}
              </MetadataListItem>
              {account.last_active_at && (
                <MetadataListItem label="Last active">
                  <DateTime value={account.last_active_at} />
                </MetadataListItem>
              )}
            </MetadataList>
            {error && <Banner status="error" title={error} />}
            {canEdit && account.active_sessions > 0 && (
              <div>
                <Button
                  label="Sign out everywhere"
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
