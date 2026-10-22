"use client";

// Marketing consent: each channel's state, how it was given and from where, with a
// dialog to change it. The newest consent wins: a change the API didn't apply says so.

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";

import { type Channel, setConsent } from "../actions";

type Consent = AdminSchemas["ConsentResponse"];

const STATES: Record<string, { label: string; variant?: "success" | "warning" | "info" }> = {
  not_subscribed: { label: "Never asked" },
  pending: { label: "Pending confirmation", variant: "info" },
  subscribed: { label: "Subscribed", variant: "success" },
  unsubscribed: { label: "Unsubscribed", variant: "warning" },
  redacted: { label: "Erased" },
};

const LEVELS: Record<string, string> = {
  single: "single opt-in",
  confirmed: "confirmed (double opt-in)",
  unknown: "level unknown",
};

/** "checkout", "the admin", "app appid_…". */
function sourceLabel(source: string | null | undefined): string | null {
  if (!source) return null;
  if (source.startsWith("app:")) return `app ${source.slice(4)}`;
  return source === "admin" ? "the admin" : source;
}

export function ConsentCard({
  customerId,
  email,
  sms,
  hasPhone,
  canEdit,
  isErased,
}: {
  customerId: string;
  email: Consent;
  sms: Consent;
  /** SMS consent needs a phone. */
  hasPhone: boolean;
  /** `customers:write`. */
  canEdit: boolean;
  /** An erased customer's consent is `redacted` and can't change. */
  isErased: boolean;
}) {
  const [editing, setEditing] = useState<Channel>();
  const rows: { channel: Channel; label: string; consent: Consent; disabled?: string }[] = [
    { channel: "email", label: "Email", consent: email },
    {
      channel: "sms",
      label: "SMS",
      consent: sms,
      disabled: hasPhone ? undefined : "Needs a phone number.",
    },
  ];
  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Marketing
          </Text>
          <Text type="supporting" color="secondary">
            Whether the customer agreed to news and offers. The newest consent wins, so a late sync
            can&apos;t undo a newer opt-out.
          </Text>
        </VStack>
        <VStack gap={3}>
          {rows.map((row, i) => {
            const state = STATES[row.consent.state] ?? { label: row.consent.state };
            const details = [
              row.consent.state !== "not_subscribed" && LEVELS[row.consent.opt_in_level],
              sourceLabel(row.consent.source) && `from ${sourceLabel(row.consent.source)}`,
            ].filter(Boolean);
            return (
              <div key={row.channel} className={i > 0 ? "list-row-divided" : undefined}>
                <VStack gap={1}>
                  <HStack gap={2} hAlign="between" vAlign="center">
                    <Text weight="medium">{row.label}</Text>
                    {canEdit && !isErased && !row.disabled && (
                      <Button
                        label="Change"
                        variant="secondary"
                        size="sm"
                        onClick={() => setEditing(row.channel)}
                      />
                    )}
                  </HStack>
                  <HStack gap={1} vAlign="center" wrap="wrap">
                    <Badge label={state.label} variant={state.variant} />
                    {details.length > 0 && (
                      <Text type="supporting" color="secondary">
                        {details.join(", ")}
                        {row.consent.consented_at && (
                          <>
                            {" · "}
                            <DateTime value={row.consent.consented_at} />
                          </>
                        )}
                      </Text>
                    )}
                  </HStack>
                  {row.disabled && (
                    <Text type="supporting" color="secondary">
                      {row.disabled}
                    </Text>
                  )}
                </VStack>
              </div>
            );
          })}
        </VStack>
      </VStack>
      {editing && (
        <ConsentDialog
          customerId={customerId}
          channel={editing}
          current={editing === "email" ? email : sms}
          onClose={() => setEditing(undefined)}
        />
      )}
    </Card>
  );
}

function ConsentDialog({
  customerId,
  channel,
  current,
  onClose,
}: {
  customerId: string;
  channel: Channel;
  current: Consent;
  onClose: () => void;
}) {
  const router = useRouter();
  const [state, setState] = useState(
    current.state === "not_subscribed" || current.state === "redacted"
      ? "subscribed"
      : current.state,
  );
  const [level, setLevel] = useState(
    current.opt_in_level === "unknown" ? "single" : current.opt_in_level,
  );
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const submit = () =>
    startTransition(async () => {
      const result = await setConsent(customerId, channel, { state, opt_in_level: level });
      if (result.error) {
        setError(result.error);
        return;
      }
      if (result.applied === false) {
        setError("A newer consent is stored for this channel; nothing changed.");
        return;
      }
      router.refresh();
      onClose();
    });
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={440} purpose="form">
      <DialogHeader
        title={`${channel === "email" ? "Email" : "SMS"} marketing`}
        subtitle="Recorded as given now, by you. A customer's own choice at checkout or through an app carries its own time."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <Selector
          label="Consent"
          options={[
            { value: "subscribed", label: "Subscribed" },
            { value: "pending", label: "Pending confirmation" },
            { value: "unsubscribed", label: "Unsubscribed" },
          ]}
          value={state}
          onChange={setState}
        />
        <Selector
          label="How it was given"
          options={[
            { value: "single", label: "Single opt-in (ticked a box, said so)" },
            { value: "confirmed", label: "Confirmed (double opt-in)" },
            { value: "unknown", label: "Unknown" },
          ]}
          value={level}
          onChange={setLevel}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button label="Save" variant="primary" isLoading={isPending} onClick={submit} />
        </HStack>
      </VStack>
    </Dialog>
  );
}
