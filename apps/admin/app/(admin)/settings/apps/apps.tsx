"use client";

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CodeBlock } from "@astryxdesign/core/CodeBlock";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
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
import { KeyIcon, PencilSquareIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { createApp, deleteApp, rotateSecret, setEnabled, updateApp } from "./actions";

type App = AdminSchemas["AppResponse"];

/** The header an app verifies, and examples of what the form takes: never translated. */
const SIGNATURE_HEADER = "Hypha-Signature";
const WEBHOOK_EXAMPLE = "https://erp.example.com/hypha";
const EVENTS_EXAMPLE = "order.*, product.updated";

/** What the last action on an app said, shown above the table. */
type Message = { text: string; isError: boolean };

export function AppsCard({ apps }: { apps: App[] }) {
  const [editing, setEditing] = useState<App | "new">();
  const [secret, setSecret] = useState<{ appKey: string; secret: string }>();
  const [message, setMessage] = useState<Message>();
  const t = useT<"apps">();

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("common.apps")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("apps.dev_card_description", { header: SIGNATURE_HEADER })}
            </Text>
          </VStack>
          <Button
            label={t("apps.dev_add_app")}
            variant="secondary"
            onClick={() => setEditing("new")}
          />
        </HStack>
        {message && (
          <Banner
            status={message.isError ? "error" : "success"}
            title={message.text}
            onDismiss={() => setMessage(undefined)}
          />
        )}
        {apps.length === 0 ? (
          <Text color="secondary">{t("apps.dev_empty")}</Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("common.app")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.dev_column_webhook")}</TableHeaderCell>
                <TableHeaderCell>{t("common.on")}</TableHeaderCell>
                <TableHeaderCell>
                  <span />
                </TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {apps.map((app) => (
                <AppRow
                  key={app.key}
                  app={app}
                  onEdit={() => setEditing(app)}
                  onSecret={(value) => setSecret({ appKey: app.key, secret: value })}
                  onMessage={setMessage}
                />
              ))}
            </TableBody>
          </Table>
        )}
      </VStack>
      {editing && (
        <AppDialog
          app={editing === "new" ? undefined : editing}
          onClose={() => setEditing(undefined)}
          onCreated={(key, value) => {
            setEditing(undefined);
            setSecret({ appKey: key, secret: value });
          }}
        />
      )}
      {secret && <SecretDialog {...secret} onClose={() => setSecret(undefined)} />}
    </Card>
  );
}

function AppRow({
  app,
  onEdit,
  onSecret,
  onMessage,
}: {
  app: App;
  onEdit: () => void;
  onSecret: (secret: string) => void;
  onMessage: (message?: Message) => void;
}) {
  const setError = (error?: string) =>
    onMessage(error ? { text: error, isError: true } : undefined);
  // Both break a live app at once, so each asks first.
  const [confirming, setConfirming] = useState<"delete" | "rotate">();
  const [isPending, startTransition] = useTransition();
  const t = useT<"apps">();

  return (
    <TableRow>
      <TableCell>
        <VStack gap={0}>
          <Link href={`/apps/${app.key}`} weight="medium">
            {app.name}
          </Link>
          <Text type="supporting" color="secondary">
            {app.key}
          </Text>
        </VStack>
      </TableCell>
      <TableCell>
        {app.webhook ? (
          <VStack gap={1}>
            <Text type="code">{app.webhook.url}</Text>
            <HStack gap={1} wrap="wrap">
              {app.subscriptions.length === 0 ? (
                <Badge label={t("apps.dev_no_events")} variant="warning" />
              ) : (
                app.subscriptions.map((pattern) => <Badge key={pattern} label={pattern} />)
              )}
            </HStack>
          </VStack>
        ) : (
          <Text color="secondary">{t("common.none")}</Text>
        )}
      </TableCell>
      <TableCell>
        <Switch
          label={t("apps.dev_use_app", { name: app.name })}
          isLabelHidden
          size="sm"
          value={app.enabled}
          changeAction={async (enabled) => setError((await setEnabled(app.key, enabled)).error)}
        />
      </TableCell>
      <TableCell>
        <HStack gap={1} hAlign="end" vAlign="center">
          {confirming === "delete" ? (
            <>
              <Text type="supporting" color="secondary">
                {t("apps.dev_delete_warning")}
              </Text>
              <Button
                label={t("common.cancel")}
                size="sm"
                variant="ghost"
                onClick={() => setConfirming(undefined)}
              />
              <Button
                label={t("apps.dev_delete_app")}
                size="sm"
                variant="destructive"
                isLoading={isPending}
                onClick={() =>
                  startTransition(async () => {
                    setError((await deleteApp(app.key)).error);
                    setConfirming(undefined);
                  })
                }
              />
            </>
          ) : confirming === "rotate" ? (
            <>
              <Text type="supporting" color="secondary">
                {t("apps.dev_rotate_warning")}
              </Text>
              <Button
                label={t("common.cancel")}
                size="sm"
                variant="ghost"
                onClick={() => setConfirming(undefined)}
              />
              <Button
                label={t("apps.dev_new_secret")}
                size="sm"
                variant="destructive"
                isLoading={isPending}
                onClick={() =>
                  startTransition(async () => {
                    const result = await rotateSecret(app.key);
                    setError(result.error);
                    if (result.secret) onSecret(result.secret);
                    setConfirming(undefined);
                  })
                }
              />
            </>
          ) : (
            <>
              <IconButton
                label={t("common.edit_named", { name: app.name })}
                icon={<PencilSquareIcon />}
                size="sm"
                variant="ghost"
                onClick={onEdit}
              />
              <IconButton
                label={t("apps.dev_new_secret_for", { name: app.name })}
                icon={<KeyIcon />}
                size="sm"
                variant="ghost"
                tooltip={t("apps.dev_new_secret_tooltip")}
                isDisabled={isPending}
                onClick={() => setConfirming("rotate")}
              />
              <IconButton
                label={t("common.delete_named_label", { name: app.name })}
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

/** Adds an app, or edits one (`app`). */
function AppDialog({
  app,
  onClose,
  onCreated,
}: {
  app?: App;
  onClose: () => void;
  onCreated: (key: string, secret: string) => void;
}) {
  const t = useT<"apps">();
  const format = useFormat();
  const [name, setName] = useState(app?.name ?? "");
  const [key, setKey] = useState<string>();
  const [webhookUrl, setWebhookUrl] = useState(app?.webhook?.url ?? "");
  const [timeout, setTimeoutMs] = useState(() => String(app?.webhook?.timeout_ms ?? 10000));
  const [patterns, setPatterns] = useState(() => app?.subscriptions.join(", ") ?? "");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const shownKey =
    app?.key ??
    key ??
    `app_${name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "")}`;
  const isComplete = name.trim() && shownKey !== "app_";

  const submit = () => {
    if (!isComplete) return;
    const timeoutMs = Number(timeout);
    if (!Number.isInteger(timeoutMs))
      return setError(t("apps.dev_timeout_invalid", { example: "10000" }));
    const input = { name, webhookUrl, timeoutMs, subscriptions: patterns };
    startTransition(async () => {
      if (app) {
        const result = await updateApp(app.key, input);
        if (result.error) setError(result.error);
        else onClose();
        return;
      }
      const result = await createApp({ key: shownKey, ...input });
      if (result.error) setError(result.error);
      else if (result.secret) onCreated(shownKey, result.secret);
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560} purpose="form">
      <DialogHeader
        title={
          app ? t("apps.dev_dialog_edit_title", { name: app.name }) : t("apps.dev_dialog_add_title")
        }
        subtitle={app ? t("apps.dev_dialog_edit_subtitle") : t("apps.dev_dialog_add_subtitle")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={t("common.name")}
          placeholder={t("apps.dev_name_placeholder")}
          value={name}
          onChange={setName}
        />
        {!app && (
          <TextInput
            label={t("common.key")}
            description={t("apps.dev_key_description", { prefix: "app_" })}
            value={shownKey}
            onChange={setKey}
          />
        )}
        <TextInput
          label={t("apps.dev_webhook_url")}
          description={t("apps.dev_webhook_url_description")}
          placeholder={WEBHOOK_EXAMPLE}
          value={webhookUrl}
          onChange={setWebhookUrl}
        />
        <TextInput
          label={t("apps.dev_events")}
          description={t("apps.dev_events_description", {
            one: "order.placed",
            kind: "product.*",
            all: "*",
          })}
          placeholder={EVENTS_EXAMPLE}
          value={patterns}
          onChange={setPatterns}
        />
        <TextInput
          label={t("apps.dev_timeout")}
          description={t("apps.dev_timeout_description", {
            min: format.number(1000),
            max: format.number(30000),
          })}
          value={timeout}
          onChange={setTimeoutMs}
          onEnter={submit}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={app ? t("common.save") : t("apps.dev_dialog_add_submit")}
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
  appKey,
  secret,
  onClose,
}: {
  appKey: string;
  secret: string;
  onClose: () => void;
}) {
  const t = useT<"apps">();
  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560}>
      <DialogHeader
        title={t("apps.dev_secret_title", { key: appKey })}
        subtitle={t("apps.dev_secret_subtitle", { header: SIGNATURE_HEADER })}
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
