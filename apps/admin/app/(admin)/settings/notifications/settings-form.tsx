"use client";

// The notification settings: branding, the sender's name and reply-to, the staff
// recipients and the order link, one draft saved by the bar at the top. The logo saves
// as it's picked, like a category's image.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import { TrashIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState, useTransition } from "react";

import { ImagePicker } from "@/components/image-picker";
import { SaveBar } from "@/components/save-bar";
import { FOOTER_MAX, STAFF_RECIPIENTS_MAX } from "@/lib/notifications";
import { useT } from "@/lib/use-t";

import { type SettingsPatch, updateNotificationSettings } from "./actions";

type Settings = AdminSchemas["NotificationSettingsResponse"];

/** A staff recipient row: an ID of its own, so rows keep their keys as they change. */
type Recipient = { id: string; email: string };

const HEX = /^#[0-9a-fA-F]{6}$/;

/** Blank text is `null` (clears the field). */
function orNull(value: string): string | null {
  return value.trim() === "" ? null : value.trim();
}

function draftOf(settings: Settings) {
  return {
    accent: settings.accent,
    footer: settings.footer ?? "",
    senderName: settings.sender_name ?? "",
    replyTo: settings.reply_to ?? "",
    orderUrl: settings.order_url ?? "",
    recipients: settings.staff_recipients.map((email): Recipient => ({
      id: `saved-${email}`,
      email,
    })),
  };
}

export function NotificationSettingsForm({
  settings,
  canWrite,
}: {
  settings: Settings;
  canWrite: boolean;
}) {
  const t = useT<"notifications">();
  const [form, setForm] = useState(() => draftOf(settings));
  const [error, setError] = useState<string>();
  const [isSaved, setIsSaved] = useState(false);
  const [isPending, startTransition] = useTransition();
  const set = (change: Partial<typeof form>) => {
    setForm((before) => ({ ...before, ...change }));
    setIsSaved(false);
  };

  // Only what changed goes in the patch, so a save never rewrites what it didn't touch.
  const saved = draftOf(settings);
  const patch: SettingsPatch = {};
  if (form.accent.toLowerCase() !== saved.accent.toLowerCase()) patch.accent = form.accent;
  if (form.footer.trim() !== saved.footer.trim()) patch.footer = orNull(form.footer);
  if (form.senderName.trim() !== saved.senderName) patch.sender_name = orNull(form.senderName);
  if (form.replyTo.trim() !== saved.replyTo) patch.reply_to = orNull(form.replyTo);
  if (form.orderUrl.trim() !== saved.orderUrl) patch.order_url = orNull(form.orderUrl);
  const recipients = form.recipients.map((r) => r.email.trim()).filter((e) => e !== "");
  if (recipients.join("\n") !== settings.staff_recipients.join("\n")) {
    patch.staff_recipients = recipients;
  }
  const isDirty = Object.keys(patch).length > 0;
  const isAccentValid = HEX.test(form.accent);

  const save = () =>
    startTransition(async () => {
      const result = await updateNotificationSettings(patch);
      setError(result.error);
      setIsSaved(!result.error);
    });

  const setRecipient = (id: string, email: string) =>
    set({ recipients: form.recipients.map((r) => (r.id === id ? { id, email } : r)) });

  return (
    <VStack gap={4}>
      <SaveBar
        isDirty={isDirty}
        isPending={isPending}
        error={error}
        canSave={isAccentValid && form.footer.length <= FOOTER_MAX}
        onSave={save}
        onDiscard={() => {
          setForm(draftOf(settings));
          setError(undefined);
        }}
      />
      {isSaved && !isDirty && <Banner status="success" title={t("notifications.settings_saved")} />}
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("notifications.branding_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("notifications.branding_description")}
            </Text>
          </VStack>
          <LogoField
            logoUrl={settings.logo_url ?? null}
            hasLogo={Boolean(settings.logo)}
            canWrite={canWrite}
          />
          <VStack gap={1}>
            <Text type="label">{t("notifications.accent_label")}</Text>
            <HStack gap={2} vAlign="center">
              <input
                type="color"
                aria-label={t("notifications.accent_picker_label")}
                value={isAccentValid ? form.accent.toLowerCase() : "#000000"}
                disabled={!canWrite}
                onChange={(e) => set({ accent: e.target.value })}
                className="colour-swatch"
              />
              <TextInput
                label={t("notifications.accent_label")}
                isLabelHidden
                width={140}
                value={form.accent}
                isDisabled={!canWrite}
                status={
                  isAccentValid
                    ? undefined
                    : { type: "error", message: t("notifications.accent_invalid") }
                }
                onChange={(accent) => set({ accent })}
              />
            </HStack>
            <Text type="supporting" color="secondary">
              {t("notifications.accent_hint")}
            </Text>
          </VStack>
          <TextArea
            label={t("notifications.footer_label")}
            description={t("notifications.footer_description")}
            isOptional
            maxLength={FOOTER_MAX}
            rows={3}
            value={form.footer}
            isDisabled={!canWrite}
            onChange={(footer) => set({ footer })}
          />
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("notifications.sender_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("notifications.sender_description", { from: settings.from })}
            </Text>
          </VStack>
          <div className="form-grid">
            <TextInput
              label={t("notifications.sender_name_label")}
              description={t("notifications.sender_name_description")}
              isOptional
              value={form.senderName}
              isDisabled={!canWrite}
              onChange={(senderName) => set({ senderName })}
            />
            <TextInput
              label={t("notifications.reply_to_label")}
              description={t("notifications.reply_to_description")}
              placeholder={t("notifications.reply_to_placeholder")}
              isOptional
              value={form.replyTo}
              isDisabled={!canWrite}
              onChange={(replyTo) => set({ replyTo })}
            />
          </div>
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("notifications.recipients_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("notifications.recipients_description", { max: STAFF_RECIPIENTS_MAX })}
            </Text>
          </VStack>
          {form.recipients.length === 0 && (
            <Text color="secondary">{t("notifications.recipients_empty")}</Text>
          )}
          {form.recipients.map((recipient, index) => (
            <HStack key={recipient.id} gap={2} vAlign="center">
              <TextInput
                label={t("notifications.recipient_label", { number: index + 1 })}
                isLabelHidden
                placeholder={t("notifications.recipient_placeholder")}
                width="100%"
                value={recipient.email}
                isDisabled={!canWrite}
                onChange={(email) => setRecipient(recipient.id, email)}
              />
              {canWrite && (
                <IconButton
                  label={
                    recipient.email
                      ? t("notifications.recipient_remove", { email: recipient.email })
                      : t("notifications.recipient_remove_blank")
                  }
                  icon={<TrashIcon />}
                  size="sm"
                  variant="ghost"
                  onClick={() =>
                    set({ recipients: form.recipients.filter((r) => r.id !== recipient.id) })
                  }
                />
              )}
            </HStack>
          ))}
          {canWrite && (
            <HStack>
              <Button
                label={t("notifications.recipient_add")}
                variant="secondary"
                isDisabled={form.recipients.length >= STAFF_RECIPIENTS_MAX}
                onClick={() =>
                  set({
                    recipients: [...form.recipients, { id: crypto.randomUUID(), email: "" }],
                  })
                }
              />
            </HStack>
          )}
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("notifications.order_link_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("notifications.order_link_description")}
            </Text>
          </VStack>
          <TextInput
            label={t("notifications.order_link_label")}
            isOptional
            placeholder={t("notifications.order_link_placeholder")}
            value={form.orderUrl}
            isDisabled={!canWrite}
            onChange={(orderUrl) => set({ orderUrl })}
          />
        </VStack>
      </Card>
    </VStack>
  );
}

/** The logo at the top of every email, from the media library; saved as it's picked. */
function LogoField({
  logoUrl,
  hasLogo,
  canWrite,
}: {
  logoUrl: string | null;
  hasLogo: boolean;
  canWrite: boolean;
}) {
  const t = useT<"notifications">();
  const [isPicking, setIsPicking] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const setLogo = (logo: string | null) =>
    startTransition(async () => {
      const result = await updateNotificationSettings({ logo });
      setError(result.error);
      if (!result.error) setIsPicking(false);
    });

  return (
    <VStack gap={2}>
      <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
        <VStack gap={0.5}>
          <Text type="label">{t("notifications.logo_label")}</Text>
          <Text type="supporting" color="secondary">
            {t("notifications.logo_description")}
          </Text>
        </VStack>
        {canWrite && (
          <HStack gap={2}>
            {hasLogo && (
              <Button
                label={t("common.remove")}
                variant="ghost"
                isLoading={isPending && !isPicking}
                onClick={() => setLogo(null)}
              />
            )}
            <Button
              label={hasLogo ? t("common.change") : t("notifications.logo_pick")}
              variant="secondary"
              onClick={() => setIsPicking(true)}
            />
          </HStack>
        )}
      </HStack>
      {error && <Banner status="error" title={error} />}
      {logoUrl ? (
        // A plain <img>: the media URLs resize, so no Next.js image pipeline.
        <img src={logoUrl} alt={t("notifications.logo_alt")} className="email-logo" />
      ) : (
        <Text color="secondary">
          {hasLogo ? t("notifications.logo_no_storage") : t("notifications.logo_none")}
        </Text>
      )}
      {isPicking && (
        <ImagePicker isSaving={isPending} onPick={setLogo} onClose={() => setIsPicking(false)} />
      )}
    </VStack>
  );
}
