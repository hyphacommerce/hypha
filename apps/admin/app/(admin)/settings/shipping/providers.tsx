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
import type { AdminSchemas } from "@hypha/sdk/admin";
import { KeyIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { createProvider, deleteProvider, rotateSecret, setAppEnabled, testProvider } from "./actions";

/** A provider's rate settings with its app's name and switch (design note 0016, EV11). */
export type RateApp = AdminSchemas["AppProviderResponse"] & { name: string; enabled: boolean };

type Provider = RateApp;

/**
 * Rate apps (design note 0015, section 5): external services Hypha asks for live rates
 * over a signed callback. Each is an app (Settings → Apps), whose signing secret is
 * shown once, when created or rotated.
 */
export function ProvidersCard({ providers }: { providers: Provider[] }) {
  const [isAdding, setIsAdding] = useState(false);
  const [secret, setSecret] = useState<{ providerKey: string; secret: string }>();
  const [message, setMessage] = useState<Message>();

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Rate apps
            </Text>
            <Text type="supporting" color="secondary">
              Services that quote live prices: carriers, or your own code in any language.
            </Text>
          </VStack>
          <Button label="Add rate app" variant="secondary" onClick={() => setIsAdding(true)} />
        </HStack>
        {message && (
          <Banner
            status={message.isError ? "error" : "success"}
            title={message.text}
            onDismiss={() => setMessage(undefined)}
          />
        )}
        {providers.length === 0 ? (
          <Text color="secondary">None yet. Built-in rates need no app.</Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>App</TableHeaderCell>
                <TableHeaderCell>Callback</TableHeaderCell>
                <TableHeaderCell>On</TableHeaderCell>
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
            ? `${provider.name} answered, with no services.`
            : `${provider.name} answered: ${services.map((s) => `${s.name} (${s.code})`).join(", ")}.`,
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
            <Badge label={`${provider.timeout_ms} ms`} />
          </HStack>
        </VStack>
      </TableCell>
      <TableCell>
        <Switch
          label={`Use ${provider.name} (its webhooks too)`}
          isLabelHidden
          size="sm"
          value={provider.enabled}
          changeAction={async (enabled) =>
            setError((await setAppEnabled(provider.key, enabled)).error)
          }
        />
      </TableCell>
      <TableCell>
        <HStack gap={1} hAlign="end" vAlign="center">
          {confirming === "delete" ? (
            <>
              <Button label="Cancel" size="sm" variant="ghost" onClick={() => setConfirming(undefined)} />
              <Button
                label="Stop using"
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
                The current secret stops working at once.
              </Text>
              <Button label="Cancel" size="sm" variant="ghost" onClick={() => setConfirming(undefined)} />
              <Button
                label="New secret"
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
                label="Test callback"
                size="sm"
                variant="secondary"
                isLoading={isTesting}
                onClick={test}
              />
              <IconButton
                label={`New secret for ${provider.name}`}
                icon={<KeyIcon />}
                size="sm"
                variant="ghost"
                tooltip="New secret: the current one stops working at once."
                isDisabled={isPending}
                onClick={() => setConfirming("rotate")}
              />
              <IconButton
                label={`Stop using ${provider.name} for rates`}
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
  const [name, setName] = useState("");
  const [key, setKey] = useState<string>();
  const [callbackUrl, setCallbackUrl] = useState("");
  const [timeout, setTimeoutMs] = useState("3000");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const shownKey =
    key ?? `app_${name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "")}`;
  const isComplete = name.trim() && shownKey !== "app_" && callbackUrl.trim();

  const submit = () => {
    if (!isComplete) return;
    const timeoutMs = Number(timeout);
    if (!Number.isInteger(timeoutMs)) return setError("Enter the timeout in milliseconds, like 3000.");
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
        title="Add rate app"
        subtitle="Hypha POSTs each rate request to the callback, signed with the app's secret: a new app's is shown next."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput label="Name" placeholder="Courier" value={name} onChange={setName} />
        <TextInput
          label="Key"
          description="How options name it: app_ and lowercase letters, digits and _. Can't change later. An existing app's key adds rates to that app."
          value={shownKey}
          onChange={setKey}
        />
        <TextInput
          label="Callback URL"
          description="https://, or http://localhost while developing."
          placeholder="https://rates.example.com/hypha"
          value={callbackUrl}
          onChange={setCallbackUrl}
          onEnter={submit}
        />
        <TextInput
          label="Timeout (ms)"
          description="Slower answers count as failures, and the zone's backup options show."
          value={timeout}
          onChange={setTimeoutMs}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Add rate app"
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
  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560}>
      <DialogHeader
        title={`Signing secret for ${providerKey}`}
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
