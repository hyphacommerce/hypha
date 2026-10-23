"use client";

// API keys: a paged list on the shared table, making a key (its token shown once), and
// revoking one.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { CodeBlock } from "@astryxdesign/core/CodeBlock";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { SegmentedControl, SegmentedControlItem } from "@astryxdesign/core/SegmentedControl";
import { Selector } from "@astryxdesign/core/Selector";
import { type TableColumn, pixel } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { KeyIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { ResourceTable } from "@/components/resource-table/resource-table";

import { createApiKey, revokeApiKey } from "./actions";

const NO_NAMES: Record<string, string> = {};
const NO_SCOPES: string[] = [];

type ApiKey = AdminSchemas["ApiKeyResponse"];

export function ApiKeys({
  page,
  channels = NO_NAMES,
  scopes = NO_SCOPES,
  canWrite = false,
}: {
  /** The page of keys; absent while it loads (the table shows its toolbar meanwhile). */
  page?: { items: ApiKey[]; next_cursor: string | null; error?: string };
  /** Channel names by ID. */
  channels?: Record<string, string>;
  /** The scopes the signed-in user holds: the most a new secret key may have. */
  scopes?: string[];
  canWrite?: boolean;
}) {
  const [isCreating, setIsCreating] = useState(false);
  const [revoking, setRevoking] = useState<ApiKey>();

  const columns: TableColumn<ApiKey>[] = [
    {
      key: "name",
      header: "Name",
      renderCell: (k) => (
        <VStack gap={0.5}>
          <Text weight="medium">{k.name}</Text>
          <Text type="supporting" color="secondary">
            {k.redacted}
          </Text>
        </VStack>
      ),
    },
    {
      key: "kind",
      header: "Kind",
      width: pixel(130),
      renderCell: (k) => <Badge label={k.kind === "secret" ? "Secret" : "Publishable"} />,
    },
    {
      key: "access",
      header: "Access",
      renderCell: (k) =>
        k.kind === "publishable"
          ? `Storefront of ${k.channel ? (channels[k.channel] ?? k.channel) : "every channel"}`
          : `${k.scopes.length} ${k.scopes.length === 1 ? "scope" : "scopes"}: ${k.scopes.join(", ")}`,
    },
    {
      key: "created_at",
      header: "Created",
      width: pixel(190),
      renderCell: (k) => <DateTime value={k.created_at} short />,
    },
    {
      key: "state",
      header: "",
      width: pixel(130),
      align: "end",
      renderCell: (k) =>
        k.revoked_at ? (
          <Badge label="Revoked" variant="neutral" />
        ) : (
          canWrite && (
            <Button label="Revoke" size="sm" variant="ghost" onClick={() => setRevoking(k)} />
          )
        ),
    },
  ];

  return (
    <VStack gap={4}>
      {canWrite && (
        <HStack hAlign="end">
          <Button label="Create key" variant="primary" onClick={() => setIsCreating(true)} />
        </HStack>
      )}
      <ResourceTable<ApiKey>
        rows={page?.items}
        nextCursor={page?.next_cursor}
        isLoading={!page}
        columns={columns}
        idKey="id"
        isSelectable={false}
        error={page?.error}
        empty={{
          icon: <KeyIcon width={32} />,
          title: "No API keys",
          description:
            "Secret keys let your own systems call the admin API; publishable keys let a storefront read what a channel sells.",
        }}
      />
      {isCreating && (
        <CreateKeyDialog channels={channels} scopes={scopes} onClose={() => setIsCreating(false)} />
      )}
      {revoking && <RevokeDialog apiKey={revoking} onClose={() => setRevoking(undefined)} />}
    </VStack>
  );
}

function CreateKeyDialog({
  channels,
  scopes,
  onClose,
}: {
  channels: Record<string, string>;
  scopes: string[];
  onClose: () => void;
}) {
  const [kind, setKind] = useState<"secret" | "publishable">("secret");
  const [name, setName] = useState("");
  const [chosen, setChosen] = useState<string[]>([]);
  const [channel, setChannel] = useState<string | undefined>(() => Object.keys(channels)[0]);
  const [token, setToken] = useState<string>();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const isComplete =
    name.trim() !== "" && (kind === "secret" ? chosen.length > 0 : Boolean(channel));

  const submit = () => {
    if (!isComplete) return;
    startTransition(async () => {
      const result =
        kind === "secret"
          ? await createApiKey({ kind, name: name.trim(), scopes: chosen })
          : await createApiKey({ kind, name: name.trim(), channel: channel ?? "" });
      if (result.error) setError(result.error);
      else setToken(result.token);
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560} purpose="form">
      <DialogHeader
        title={token ? `${name.trim()} is ready` : "Create API key"}
        subtitle={
          token
            ? "Copy it now: it isn't shown again."
            : "Keys act on their own, with the scopes they're given."
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {token ? (
          <>
            <CodeBlock
              code={token}
              title={kind === "secret" ? "Secret key" : "Publishable key"}
              isWrapped
            />
            {kind === "publishable" && (
              <Text type="supporting" color="secondary">
                For a storefront, it goes in its STOREFRONT_KEY setting (apps/storefront/.dev.vars
                when you run it locally).
              </Text>
            )}
            {kind === "secret" && (
              <Banner
                status="warning"
                title="Keep it secret"
                description="It works like a staff login with these scopes: store it in your server's secrets, never in a browser."
              />
            )}
          </>
        ) : (
          <>
            <SegmentedControl
              label="Kind"
              value={kind}
              onChange={(value) => {
                if (value === "secret" || value === "publishable") setKind(value);
              }}
            >
              <SegmentedControlItem value="secret" label="Secret" />
              <SegmentedControlItem value="publishable" label="Publishable" />
            </SegmentedControl>
            <TextInput
              label="Name"
              description="Shown to staff, so you know what uses it."
              placeholder={kind === "secret" ? "ERP sync" : "Main storefront"}
              value={name}
              onChange={setName}
            />
            {kind === "secret" ? (
              <MultiSelector
                label="Scopes"
                description="Only scopes you hold yourself."
                options={scopes.map((s) => ({ value: s, label: s }))}
                value={chosen}
                onChange={setChosen}
                hasSearch
                hasSelectAll
                triggerDisplay="count"
              />
            ) : (
              <Selector
                label="Channel"
                description="The storefront reads and sells what this channel publishes."
                options={Object.entries(channels).map(([value, label]) => ({ value, label }))}
                value={channel}
                onChange={setChannel}
              />
            )}
          </>
        )}
        <HStack gap={2} hAlign="end">
          {token ? (
            <Button label="Done" variant="primary" onClick={onClose} />
          ) : (
            <>
              <Button label="Cancel" variant="ghost" onClick={onClose} />
              <Button
                label="Create key"
                variant="primary"
                isLoading={isPending}
                isDisabled={!isComplete}
                onClick={submit}
              />
            </>
          )}
        </HStack>
      </VStack>
    </Dialog>
  );
}

function RevokeDialog({ apiKey, onClose }: { apiKey: ApiKey; onClose: () => void }) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={`Revoke ${apiKey.name}?`}
      description={
        error ??
        "Every call with this key fails from now on. This can't be undone: make a new key instead."
      }
      actionLabel="Revoke key"
      isActionLoading={isPending}
      onAction={() =>
        startTransition(async () => {
          const result = await revokeApiKey(apiKey.id);
          if (result.error) setError(result.error);
          else onClose();
        })
      }
    />
  );
}
