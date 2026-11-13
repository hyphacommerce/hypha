"use client";

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CodeBlock } from "@astryxdesign/core/CodeBlock";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Switch } from "@astryxdesign/core/Switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { KeyIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import {
  createProvider,
  deleteProvider,
  rotateSecret,
  setAppEnabled,
  testProvider,
} from "./actions";

/** A provider's rate settings with its app's name and switch. */
export type RateApp = AdminSchemas["AppProviderResponse"] & {
  name: string;
  /** `null` when the staff member can't read apps. */
  enabled: boolean | null;
};

type Provider = RateApp;

/**
 * Rate apps: external services Hypha asks for live rates over a signed callback. Each
 * is an app (Settings → Apps), whose signing secret is shown once, when created or
 * rotated.
 */
export function ProvidersCard({ providers }: { providers: Provider[] }) {
  const t = useT<"shipping">();
  const [isAdding, setIsAdding] = useState(false);
  const [secret, setSecret] = useState<{ providerKey: string; secret: string }>();
  const [message, setMessage] = useState<Message>();

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("shipping.apps_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("shipping.apps_description")}
            </Text>
          </VStack>
          <Button
            label={t("shipping.apps_add")}
            variant="secondary"
            onClick={() => setIsAdding(true)}
          />
        </HStack>
        {message && (
          <Banner
            status={message.isError ? "error" : "success"}
            title={message.text}
            onDismiss={() => setMessage(undefined)}
          />
        )}
        {providers.length === 0 ? (
          <Text color="secondary">{t("shipping.apps_empty")}</Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("common.app")}</TableHeaderCell>
                <TableHeaderCell>{t("shipping.apps_column_callback")}</TableHeaderCell>
                <TableHeaderCell>{t("common.on")}</TableHeaderCell>
                <TableHeaderCell>
                  <span />
                </TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {providers.map((provider) => (
                <ProviderRow
                  key={provider.key}
                  provider={provider}
                  onSecret={(value) => setSecret({ providerKey: provider.key, secret: value })}
                  onMessage={setMessage}
                />
              ))}
            </TableBody>
          </Table>
        )}
      </VStack>
      {isAdding && (
        <AddProviderDialog
          onClose={() => setIsAdding(false)}
          onCreated={(key, value) => {
            setIsAdding(false);
            setSecret({ providerKey: key, secret: value });
          }}
        />
      )}
      {secret && <SecretDialog {...secret} onClose={() => setSecret(undefined)} />}
    </Card>
  );
}

/** What the last action on an app said, shown above the table. */
type Message = { text: string; isError: boolean };

function ProviderRow({
  provider,
  onSecret,
  onMessage,
}: {
  provider: Provider;
  onSecret: (secret: string) => void;
  onMessage: (message?: Message) => void;
}) {
  const t = useT<"shipping">();
  const format = useFormat();
  const setError = (error?: string) =>
    onMessage(error ? { text: error, isError: true } : undefined);
  // Both actions break a live app at once, so each asks first.
  const [confirming, setConfirming] = useState<"delete" | "rotate">();
  const [isPending, startTransition] = useTransition();
  const [isTesting, startTest] = useTransition();

  const test = () =>
    startTest(async () => {
      const result = await testProvider(provider.key);
      if (result.error) return setError(result.error);
      const services = result.services ?? [];
      onMessage({
        isError: false,
        text:
          services.length === 0
            ? t("shipping.app_answered_none", { app: provider.name })
            : t("shipping.app_answered", {
                app: provider.name,
                services: format.list(
                  services.map((s) => `${s.name} (${s.code})`),
                  "unit",
                ),
              }),
      });
    });

  return (
    <TableRow>
      <TableCell>
        <VStack gap={0}>
          <Text>{provider.name}</Text>
          <Text type="supporting" color="secondary">
            {provider.key}
          </Text>
        </VStack>
      </TableCell>
      <TableCell>
        <VStack gap={1}>
          <Text type="code">{provider.callback_url}</Text>
          <HStack gap={1}>
            <Badge label={t("shipping.app_timeout", { ms: String(provider.timeout_ms) })} />
          </HStack>
        </VStack>
      </TableCell>
      <TableCell>
        {provider.enabled !== null && (
          <Switch
            label={t("shipping.app_use", { app: provider.name })}
            isLabelHidden
            size="sm"
            value={provider.enabled}
            changeAction={async (enabled) =>
              setError((await setAppEnabled(provider.key, enabled)).error)
            }
          />
        )}
      </TableCell>
      <TableCell>
        <HStack gap={1} hAlign="end" vAlign="center">
          {confirming === "delete" ? (
            <>
              <Button
                label={t("common.cancel")}
                size="sm"
                variant="ghost"
                onClick={() => setConfirming(undefined)}
              />
              <Button
                label={t("shipping.app_stop_using")}
                size="sm"
                variant="destructive"
                isLoading={isPending}
                onClick={() =>
                  startTransition(async () => {
                    setError((await deleteProvider(provider.key)).error);
                    setConfirming(undefined);
                  })
                }
              />
            </>
          ) : confirming === "rotate" ? (
            <>
              <Text type="supporting" color="secondary">
                {t("shipping.app_rotate_warning")}
              </Text>
              <Button
                label={t("common.cancel")}
                size="sm"
                variant="ghost"
                onClick={() => setConfirming(undefined)}
              />
              <Button
                label={t("shipping.app_new_secret")}
                size="sm"
                variant="destructive"
                isLoading={isPending}
                onClick={() =>
                  startTransition(async () => {
                    const result = await rotateSecret(provider.key);
                    setError(result.error);
                    if (result.secret) onSecret(result.secret);
                    setConfirming(undefined);
                  })
                }
              />
            </>
          ) : (
            <>
              <Button
                label={t("shipping.app_test")}
                size="sm"
                variant="secondary"
                isLoading={isTesting}
                onClick={test}
              />
              <IconButton
                label={t("shipping.app_new_secret_for", { app: provider.name })}
                icon={<KeyIcon />}
                size="sm"
                variant="ghost"
                tooltip={t("shipping.app_new_secret_tooltip")}
                isDisabled={isPending}
                onClick={() => setConfirming("rotate")}
              />
              <IconButton
                label={t("shipping.app_stop_using_for_rates", { app: provider.name })}
                icon={<TrashIcon />}
                size="sm"
                variant="ghost"
                onClick={() => setConfirming("delete")}
              />
            </>
          )}
        </HStack>
      </TableCell>
    </TableRow>
  );
}

function AddProviderDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (key: string, secret: string) => void;
}) {
  const t = useT<"shipping">();
  const [name, setName] = useState("");
  const [key, setKey] = useState<string>();
  const [callbackUrl, setCallbackUrl] = useState("");
  const [timeout, setTimeoutMs] = useState("3000");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const shownKey =
    key ??
    `app_${name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "")}`;
  const isComplete = name.trim() && shownKey !== "app_" && callbackUrl.trim();

  const submit = () => {
    if (!isComplete) return;
    const timeoutMs = Number(timeout);
    if (!Number.isInteger(timeoutMs)) return setError(t("shipping.app_timeout_invalid"));
    startTransition(async () => {
      const result = await createProvider({ key: shownKey, name, callbackUrl, timeoutMs });
      if (result.error) setError(result.error);
      else if (result.secret) onCreated(shownKey, result.secret);
      // An existing app: its secret stays as it was.
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={520} purpose="form">
      <DialogHeader
        title={t("shipping.app_dialog_title")}
        subtitle={t("shipping.app_dialog_subtitle")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={t("common.name")}
          placeholder={t("shipping.app_name_placeholder")}
          value={name}
          onChange={setName}
        />
        <TextInput
          label={t("common.key")}
          description={t("shipping.app_key_description")}
          value={shownKey}
          onChange={setKey}
        />
        <TextInput
          label={t("shipping.app_callback_label")}
          description={t("shipping.app_callback_description")}
          placeholder={t("shipping.app_callback_placeholder")}
          value={callbackUrl}
          onChange={setCallbackUrl}
          onEnter={submit}
        />
        <TextInput
          label={t("shipping.app_timeout_label")}
          description={t("shipping.app_timeout_description")}
          value={timeout}
          onChange={setTimeoutMs}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("shipping.app_dialog_submit")}
            variant="primary"
            isLoading={isPending}
            isDisabled={!isComplete}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

function SecretDialog({
  providerKey,
  secret,
  onClose,
}: {
  providerKey: string;
  secret: string;
  onClose: () => void;
}) {
  const t = useT<"shipping">();
  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560}>
      <DialogHeader
        title={t("shipping.app_secret_title", { key: providerKey })}
        subtitle={t("shipping.app_secret_subtitle")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        <CodeBlock code={secret} />
        <HStack hAlign="end">
          <Button label={t("common.done")} variant="primary" onClick={onClose} />
        </HStack>
      </VStack>
    </Dialog>
  );
}
