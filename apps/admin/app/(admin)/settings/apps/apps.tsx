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
import type { AdminSchemas } from "@hypha/sdk/admin";
import { KeyIcon, PencilSquareIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { createApp, deleteApp, rotateSecret, setEnabled, updateApp } from "./actions";

type App = AdminSchemas["AppResponse"];

/** What the last action on an app said, shown above the table. */
type Message = { text: string; isError: boolean };

export function AppsCard({ apps }: { apps: App[] }) {
  const [editing, setEditing] = useState<App | "new">();
  const [secret, setSecret] = useState<{ appKey: string; secret: string }>();
  const [message, setMessage] = useState<Message>();

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Apps
            </Text>
            <Text type="supporting" color="secondary">
              Your ERP, a fulfilment service, or your own code: anything that verifies the
              Hypha-Signature header.
            </Text>
          </VStack>
          <Button label="Add app" variant="secondary" onClick={() => setEditing("new")} />
        </HStack>
        {message && (
          <Banner
            status={message.isError ? "error" : "success"}
            title={message.text}
            onDismiss={() => setMessage(undefined)}
          />
        )}
        {apps.length === 0 ? (
          <Text color="secondary">No apps yet.</Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>App</TableHeaderCell>
                <TableHeaderCell>Webhook</TableHeaderCell>
                <TableHeaderCell>On</TableHeaderCell>
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

  return (
    <TableRow>
      <TableCell>
        <VStack gap={0}>
          <Link href={`/settings/apps/${app.key}`} weight="medium">
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
                <Badge label="No events" variant="warning" />
              ) : (
                app.subscriptions.map((pattern) => <Badge key={pattern} label={pattern} />)
              )}
            </HStack>
          </VStack>
        ) : (
          <Text color="secondary">None</Text>
        )}
      </TableCell>
      <TableCell>
        <Switch
          label={`Use ${app.name}`}
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
                Its rate settings go too.
              </Text>
              <Button
                label="Cancel"
                size="sm"
                variant="ghost"
                onClick={() => setConfirming(undefined)}
              />
              <Button
                label="Delete app"
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
                The current secret stops working at once.
              </Text>
              <Button
                label="Cancel"
                size="sm"
                variant="ghost"
                onClick={() => setConfirming(undefined)}
              />
              <Button
                label="New secret"
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
                label={`Edit ${app.name}`}
                icon={<PencilSquareIcon />}
                size="sm"
                variant="ghost"
                onClick={onEdit}
              />
              <IconButton
                label={`New secret for ${app.name}`}
                icon={<KeyIcon />}
                size="sm"
                variant="ghost"
                tooltip="New secret: the current one stops working at once."
                isDisabled={isPending}
                onClick={() => setConfirming("rotate")}
              />
              <IconButton
                label={`Delete ${app.name}`}
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
  const [name, setName] = useState(app?.name ?? "");
  const [key, setKey] = useState<string>();
  const [webhookUrl, setWebhookUrl] = useState(app?.webhook?.url ?? "");
  const [timeout, setTimeoutMs] = useState(String(app?.webhook?.timeout_ms ?? 10000));
  const [patterns, setPatterns] = useState(app?.subscriptions.join(", ") ?? "");
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
      return setError("Enter the timeout in milliseconds, like 10000.");
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
        title={app ? `Edit ${app.name}` : "Add app"}
        subtitle={
          app
            ? "Changes apply to the next events; ones already on their way keep going."
            : "Hypha signs every call to it with a secret it shows you next."
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput label="Name" placeholder="ERP" value={name} onChange={setName} />
        {!app && (
          <TextInput
            label="Key"
            description="app_ and lowercase letters, digits and _. Can't change later."
            value={shownKey}
            onChange={setKey}
          />
        )}
        <TextInput
          label="Webhook URL"
          description="Where Hypha POSTs events: https://, or http://localhost while developing. Leave empty for none."
          placeholder="https://erp.example.com/hypha"
          value={webhookUrl}
          onChange={setWebhookUrl}
        />
        <TextInput
          label="Events"
          description="Event types, separated by commas: order.placed, a whole kind like product.*, or * for all."
          placeholder="order.*, product.updated"
          value={patterns}
          onChange={setPatterns}
        />
        <TextInput
          label="Timeout (ms)"
          description="1,000 to 30,000. Slower answers count as failures and are retried."
          value={timeout}
          onChange={setTimeoutMs}
          onEnter={submit}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={app ? "Save" : "Add app"}
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
  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560}>
      <DialogHeader
        title={`Signing secret for ${appKey}`}
        subtitle="Copy it now: it isn't shown again. The app checks the Hypha-Signature header with it."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        <CodeBlock code={secret} />
        <HStack hAlign="end">
          <Button label="Done" variant="primary" onClick={onClose} />
        </HStack>
      </VStack>
    </Dialog>
  );
}
