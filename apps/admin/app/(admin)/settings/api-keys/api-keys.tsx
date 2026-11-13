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
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

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
  const t = useT<"api_keys">();
  const format = useFormat();

  const columns: TableColumn<ApiKey>[] = [
    {
      key: "name",
      header: t("common.name"),
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
      header: t("common.kind"),
      width: pixel(130),
      renderCell: (k) => (
        <Badge
          label={k.kind === "secret" ? t("api_keys.kind_secret") : t("api_keys.kind_publishable")}
        />
      ),
    },
    {
      key: "access",
      header: t("api_keys.access"),
      renderCell: (k) =>
        k.kind === "publishable"
          ? k.channel
            ? t("api_keys.access_storefront", { channel: channels[k.channel] ?? k.channel })
            : t("api_keys.access_storefront_every")
          : t("api_keys.access_scopes", {
              count: k.scopes.length,
              scopes: format.list(k.scopes, "unit"),
            }),
    },
    {
      key: "created_at",
      header: t("common.created"),
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
          <Badge label={t("api_keys.revoked")} variant="neutral" />
        ) : (
          canWrite && (
            <Button
              label={t("common.revoke")}
              size="sm"
              variant="ghost"
              onClick={() => setRevoking(k)}
            />
          )
        ),
    },
  ];

  return (
    <VStack gap={4}>
      {canWrite && (
        <HStack hAlign="end">
          <Button
            label={t("api_keys.create_key")}
            variant="primary"
            onClick={() => setIsCreating(true)}
          />
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
          title: t("api_keys.empty_title"),
          description: t("api_keys.empty_description"),
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
  const t = useT<"api_keys">();
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
        title={
          token
            ? t("api_keys.created_title", { name: name.trim() })
            : t("api_keys.create_dialog_title")
        }
        subtitle={token ? t("api_keys.created_subtitle") : t("api_keys.create_dialog_subtitle")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {token ? (
          <>
            <CodeBlock
              code={token}
              title={
                kind === "secret"
                  ? t("api_keys.created_secret_key")
                  : t("api_keys.created_publishable_key")
              }
              isWrapped
            />
            {kind === "publishable" && (
              <Text type="supporting" color="secondary">
                {t("api_keys.created_publishable_hint", {
                  setting: "STOREFRONT_KEY",
                  file: "apps/storefront/.dev.vars",
                })}
              </Text>
            )}
            {kind === "secret" && (
              <Banner
                status="warning"
                title={t("api_keys.created_secret_warning_title")}
                description={t("api_keys.created_secret_warning_description")}
              />
            )}
          </>
        ) : (
          <>
            <SegmentedControl
              label={t("common.kind")}
              value={kind}
              onChange={(value) => {
                if (value === "secret" || value === "publishable") setKind(value);
              }}
            >
              <SegmentedControlItem value="secret" label={t("api_keys.create_kind_secret")} />
              <SegmentedControlItem
                value="publishable"
                label={t("api_keys.create_kind_publishable")}
              />
            </SegmentedControl>
            <TextInput
              label={t("common.name")}
              description={t("api_keys.name_description")}
              placeholder={
                kind === "secret"
                  ? t("api_keys.name_placeholder_secret")
                  : t("api_keys.name_placeholder_publishable")
              }
              value={name}
              onChange={setName}
            />
            {kind === "secret" ? (
              <MultiSelector
                label={t("common.scopes")}
                description={t("api_keys.scopes_description")}
                options={scopes.map((s) => ({ value: s, label: s }))}
                value={chosen}
                onChange={setChosen}
                hasSearch
                hasSelectAll
                triggerDisplay="count"
              />
            ) : (
              <Selector
                label={t("common.channel")}
                description={t("api_keys.channel_description")}
                options={Object.entries(channels).map(([value, label]) => ({ value, label }))}
                value={channel}
                onChange={setChannel}
              />
            )}
          </>
        )}
        <HStack gap={2} hAlign="end">
          {token ? (
            <Button label={t("common.done")} variant="primary" onClick={onClose} />
          ) : (
            <>
              <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
              <Button
                label={t("api_keys.create_key")}
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
  const t = useT<"api_keys">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={t("api_keys.revoke_dialog_title", { name: apiKey.name })}
      description={error ?? t("api_keys.revoke_dialog_description")}
      actionLabel={t("api_keys.revoke_dialog_action")}
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
