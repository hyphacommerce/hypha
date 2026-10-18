"use client";

// The order's emails (design note 0028, §7): what was sent, to whom, when, and why one
// failed or was skipped, with Resend.

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useToast } from "@astryxdesign/core/Toast";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useState } from "react";

import { DateTime } from "@/components/date-time";
import { MESSAGE_STATUS, templateName } from "@/lib/notifications";

import { resendNotification } from "../actions";

type Message = AdminSchemas["NotificationResponse"];

export function EmailsCard({
  orderId,
  messages,
  canResend,
}: {
  orderId: string;
  /** Newest first, as the API lists them. */
  messages: Message[];
  canResend: boolean;
}) {
  const [error, setError] = useState<string>();
  const [resending, setResending] = useState<string>();
  const toast = useToast();

  const resend = async (message: Message) => {
    setResending(message.id);
    try {
      const result = await resendNotification(orderId, message.id);
      setError(result.error);
      if (!result.error) toast({ body: `Sent again to ${message.to}` });
    } finally {
      setResending(undefined);
    }
  };

  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          Emails
        </Text>
        {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
        {messages.length === 0 ? (
          <Text color="secondary">No emails about this order yet.</Text>
        ) : (
          <VStack gap={3}>
            {messages.map((message) => {
              const status = MESSAGE_STATUS[message.status] ?? {
                label: message.status,
                variant: "neutral" as const,
              };
              return (
                <HStack key={message.id} gap={3} hAlign="between" vAlign="start" wrap="wrap">
                  <VStack gap={0.5}>
                    <HStack gap={2} vAlign="center" wrap="wrap">
                      <Text weight="medium">{templateName(message.template)}</Text>
                      <Badge label={status.label} variant={status.variant} />
                    </HStack>
                    <Text type="supporting" color="secondary">
                      To {message.to} ·{" "}
                      <DateTime value={message.sent_at ?? message.created_at} short />
                      {message.attempts > 1 ? ` · ${message.attempts} attempts` : ""}
                    </Text>
                    {message.error &&
                      (message.status === "failed" || message.status === "skipped") && (
                        <Text type="supporting" color="secondary">
                          {message.status === "failed" ? "Failed: " : "Skipped: "}
                          {message.error}
                        </Text>
                      )}
                  </VStack>
                  {canResend && message.status !== "sending" && (
                    <Button
                      label="Resend"
                      variant="ghost"
                      size="sm"
                      isLoading={resending === message.id}
                      isDisabled={resending !== undefined && resending !== message.id}
                      onClick={() => resend(message)}
                    />
                  )}
                </HStack>
              );
            })}
          </VStack>
        )}
      </VStack>
    </Card>
  );
}
