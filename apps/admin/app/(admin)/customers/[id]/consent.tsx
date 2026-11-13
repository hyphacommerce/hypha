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
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import type { KeyIn, TFor } from "@/lib/messages";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { type Channel, setConsent } from "../actions";

type Consent = AdminSchemas["ConsentResponse"];

const STATES: Record<
  string,
  { label: KeyIn<"customers">; variant?: "success" | "warning" | "info" }
> = {
  not_subscribed: { label: "customers.consent_state_not_subscribed" },
  pending: { label: "customers.consent_state_pending", variant: "info" },
  subscribed: { label: "customers.consent_state_subscribed", variant: "success" },
  unsubscribed: { label: "customers.consent_state_unsubscribed", variant: "warning" },
  redacted: { label: "customers.consent_state_redacted" },
};

const LEVELS = ["single", "confirmed", "unknown"] as const;

/** Where a consent can come from besides an app, as the API names them. */
const SOURCES = ["admin", "checkout", "storefront", "import", "system"] as const;

/** "single opt-in"; nothing for a level the admin doesn't know. */
function levelLabel(t: TFor<"customers">, level: string): string | null {
  const known = LEVELS.find((l) => l === level);
  return known ? t(`customers.consent_level_${known}`) : null;
}

/** "from checkout", "from the admin", "from app appid_…". */
function sourceLabel(t: TFor<"customers">, source: string | null | undefined): string | null {
  if (!source) return null;
  if (source.startsWith("app:")) return t("customers.consent_from_app", { app: source.slice(4) });
  const known = SOURCES.find((s) => s === source);
  // A source the admin doesn't know is shown as the API names it.
  return known ? t(`customers.consent_from_${known}`) : t("customers.consent_from", { source });
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
  const t = useT<"customers">();
  const format = useFormat();
  const [editing, setEditing] = useState<Channel>();
  const rows: { channel: Channel; label: string; consent: Consent; disabled?: string }[] = [
    { channel: "email", label: t("common.email"), consent: email },
    {
      channel: "sms",
      label: t("customers.consent_channel_sms"),
      consent: sms,
      disabled: hasPhone ? undefined : t("customers.consent_needs_phone"),
    },
  ];
  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("customers.consent_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("customers.consent_description")}
          </Text>
        </VStack>
        <VStack gap={3}>
          {rows.map((row, i) => {
            const state = Object.hasOwn(STATES, row.consent.state)
              ? STATES[row.consent.state]
              : undefined;
            const details = [
              row.consent.state === "not_subscribed"
                ? null
                : levelLabel(t, row.consent.opt_in_level),
              sourceLabel(t, row.consent.source),
            ].filter((detail) => detail !== null);
            return (
              <div key={row.channel} className={i > 0 ? "list-row-divided" : undefined}>
                <VStack gap={1}>
                  <HStack gap={2} hAlign="between" vAlign="center">
                    <Text weight="medium">{row.label}</Text>
                    {canEdit && !isErased && !row.disabled && (
                      <Button
                        label={t("common.change")}
                        variant="secondary"
                        size="sm"
                        onClick={() => setEditing(row.channel)}
                      />
                    )}
                  </HStack>
                  <HStack gap={1} vAlign="center" wrap="wrap">
                    <Badge
                      label={state ? t(state.label) : row.consent.state}
                      variant={state?.variant}
                    />
                    {details.length > 0 && (
                      <Text type="supporting" color="secondary">
                        {format.list(details, "unit")}
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
  const t = useT<"customers">();
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
        setError(t("customers.consent_not_applied"));
        return;
      }
      router.refresh();
      onClose();
    });
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={440} purpose="form">
      <DialogHeader
        title={t(`customers.consent_dialog_title_${channel}`)}
        subtitle={t("customers.consent_dialog_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <Selector
          label={t("customers.consent_dialog_state")}
          options={[
            { value: "subscribed", label: t("customers.consent_state_subscribed") },
            { value: "pending", label: t("customers.consent_state_pending") },
            { value: "unsubscribed", label: t("customers.consent_state_unsubscribed") },
          ]}
          value={state}
          onChange={setState}
        />
        <Selector
          label={t("customers.consent_dialog_level")}
          options={[
            { value: "single", label: t("customers.consent_dialog_level_single") },
            { value: "confirmed", label: t("customers.consent_dialog_level_confirmed") },
            { value: "unknown", label: t("common.unknown") },
          ]}
          value={level}
          onChange={setLevel}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.save")}
            variant="primary"
            isLoading={isPending}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
