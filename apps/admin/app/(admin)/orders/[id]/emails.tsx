"use client";

// The order's emails: what was sent, to whom, when, and why one failed or was skipped,
// with Resend (which queues a new one for the send job).

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useToast } from "@astryxdesign/core/Toast";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState } from "react";

import { DateTime } from "@/components/date-time";
import { messageStatus, templateName } from "@/lib/notifications";
import { useT } from "@/lib/use-t";

import { resendNotification } from "../actions";

type Message = AdminSchemas["NotificationResponse"];

/** The statuses whose error has a sentence of its own; any other reads "Error: …". */
const ERROR_STATUSES = ["failed", "skipped", "queued"] as const;

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
  const t = useT<"orders" | "notifications">();

  const resend = async (message: Message) => {
    setResending(message.id);
    try {
      const result = await resendNotification(orderId, message.id);
      setError(result.error);
      if (!result.error) toast({ body: t("orders.emails_queued_again", { to: message.to }) });
    } finally {
      setResending(undefined);
    }
  };

  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          {t("orders.emails_title")}
        </Text>
        {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
        {messages.length === 0 ? (
          <Text color="secondary">{t("orders.emails_empty")}</Text>
        ) : (
          <VStack gap={3}>
            {messages.map((message) => {
              const status = messageStatus(t, message.status);
              const sent = (
                <DateTime key="time" value={message.sent_at ?? message.created_at} short />
              );
              const errorStatus = ERROR_STATUSES.find((s) => s === message.status) ?? "other";
              return (
                <HStack key={message.id} gap={3} hAlign="between" vAlign="start" wrap="wrap">
                  <VStack gap={0.5}>
                    <HStack gap={2} vAlign="center" wrap="wrap">
                      <Text weight="medium">{templateName(t, message.template)}</Text>
                      <Badge label={status.label} variant={status.variant} />
                    </HStack>
                    <Text type="supporting" color="secondary">
                      {message.attempts > 1
                        ? t.rich("orders.emails_sent_to_attempts", {
                            to: message.to,
                            time: sent,
                            attempts: message.attempts,
                          })
                        : t.rich("orders.emails_sent_to", { to: message.to, time: sent })}
                    </Text>
                    {message.error && message.status !== "sent" && (
                      <Text type="supporting" color="secondary">
                        {message.status === "queued" && message.next_attempt_at
                          ? t.rich("orders.emails_error_queued_next", {
                              error: message.error,
                              time: <DateTime key="time" value={message.next_attempt_at} short />,
                            })
                          : t(`orders.emails_error_${errorStatus}`, { error: message.error })}
                      </Text>
                    )}
                    {message.note && (
                      <Text type="supporting" color="secondary">
                        {message.note}
                      </Text>
                    )}
                  </VStack>
                  {canResend && message.status !== "sending" && message.status !== "queued" && (
                    <Button
                      label={t("common.resend")}
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
