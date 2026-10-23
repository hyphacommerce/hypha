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
      {isSaved && !isDirty && <Banner status="success" title="Saved." />}
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Branding
            </Text>
            <Text type="supporting" color="secondary">
              Every email shows these, through the email layout.
            </Text>
          </VStack>
          <LogoField
            logoUrl={settings.logo_url ?? null}
            hasLogo={Boolean(settings.logo)}
            canWrite={canWrite}
          />
          <VStack gap={1}>
            <Text type="label">Accent colour</Text>
            <HStack gap={2} vAlign="center">
              <input
                type="color"
                aria-label="Pick the accent colour"
                value={isAccentValid ? form.accent.toLowerCase() : "#000000"}
                disabled={!canWrite}
                onChange={(e) => set({ accent: e.target.value })}
                className="colour-swatch"
              />
              <TextInput
                label="Accent colour"
                isLabelHidden
                width={140}
                value={form.accent}
                isDisabled={!canWrite}
                status={isAccentValid ? undefined : { type: "error", message: "Like #1a2b3c" }}
                onChange={(accent) => set({ accent })}
              />
            </HStack>
            <Text type="supporting" color="secondary">
              Buttons and links.
            </Text>
          </VStack>
          <TextArea
            label="Footer"
            description="Plain text under every email: your address, legal lines."
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
              Sender
            </Text>
            <Text type="supporting" color="secondary">
              The from address is the deployment&apos;s ({settings.from}); the name goes with it.
            </Text>
          </VStack>
          <div className="form-grid">
            <TextInput
              label="Sender name"
              description="Blank: the store's name."
              isOptional
              value={form.senderName}
              isDisabled={!canWrite}
              onChange={(senderName) => set({ senderName })}
            />
            <TextInput
              label="Reply-to"
              description="Where customers' replies go."
              placeholder="support@example.com"
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
              Staff recipients
            </Text>
            <Text type="supporting" color="secondary">
              Who gets the New order email. Up to {STAFF_RECIPIENTS_MAX} addresses.
            </Text>
          </VStack>
          {form.recipients.length === 0 && <Text color="secondary">Nobody yet.</Text>}
          {form.recipients.map((recipient, index) => (
            <HStack key={recipient.id} gap={2} vAlign="center">
              <TextInput
                label={`Recipient ${index + 1}`}
                isLabelHidden
                placeholder="orders@example.com"
                width="100%"
                value={recipient.email}
                isDisabled={!canWrite}
                onChange={(email) => setRecipient(recipient.id, email)}
              />
              {canWrite && (
                <IconButton
                  label={`Remove ${recipient.email || "recipient"}`}
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
                label="Add recipient"
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
              Order link
            </Text>
            <Text type="supporting" color="secondary">
              The customer&apos;s order page, linked from their emails. {"{order}"} becomes the
              order&apos;s ID and {"{token}"} its access token, which lets the page show the order
              on any device. Blank: emails link to no order.
            </Text>
          </VStack>
          <TextInput
            label="Order page URL"
            isOptional
            placeholder="https://shop.example.com/order/{order}?token={token}"
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
          <Text type="label">Logo</Text>
          <Text type="supporting" color="secondary">
            A public image from the media library. It can&apos;t be deleted there while it&apos;s
            the logo.
          </Text>
        </VStack>
        {canWrite && (
          <HStack gap={2}>
            {hasLogo && (
              <Button
                label="Remove"
                variant="ghost"
                isLoading={isPending && !isPicking}
                onClick={() => setLogo(null)}
              />
            )}
            <Button
              label={hasLogo ? "Change" : "Pick from library"}
              variant="secondary"
              onClick={() => setIsPicking(true)}
            />
          </HStack>
        )}
      </HStack>
      {error && <Banner status="error" title={error} />}
      {logoUrl ? (
        // A plain <img>: the media URLs resize, so no Next.js image pipeline.
        <img src={logoUrl} alt="The emails' logo" className="email-logo" />
      ) : (
        <Text color="secondary">{hasLogo ? "Media storage isn't set up." : "No logo."}</Text>
      )}
      {isPicking && (
        <ImagePicker isSaving={isPending} onPick={setLogo} onClose={() => setIsPicking(false)} />
      )}
    </VStack>
  );
}
