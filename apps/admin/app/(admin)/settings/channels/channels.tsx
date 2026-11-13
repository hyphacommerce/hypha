"use client";

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CodeBlock } from "@astryxdesign/core/CodeBlock";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
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
import { KeyIcon, PencilSquareIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { useT } from "@/lib/use-t";

import { type ChannelKind, createChannel, createStorefrontKey, renameChannel } from "./actions";

type Channel = AdminSchemas["ChannelResponse"];

const KINDS: ChannelKind[] = ["online_store", "agentic", "custom"];

/** The kind `value` names, if it's one this admin knows. */
function knownKind(value: string): ChannelKind | undefined {
  return KINDS.find((kind) => kind === value);
}

export function ChannelsCard({ channels }: { channels: Channel[] }) {
  const t = useT<"settings">();
  const [editing, setEditing] = useState<Channel | "new">();
  const [keyFor, setKeyFor] = useState<Channel>();

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("common.channels")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("settings.channels_card_description")}
            </Text>
          </VStack>
          <Button
            label={t("settings.channels_add")}
            variant="secondary"
            onClick={() => setEditing("new")}
          />
        </HStack>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHeaderCell>{t("common.channel")}</TableHeaderCell>
              <TableHeaderCell>{t("settings.channels_column_kind")}</TableHeaderCell>
              <TableHeaderCell>{t("common.created")}</TableHeaderCell>
              <TableHeaderCell>
                <span />
              </TableHeaderCell>
            </TableRow>
          </TableHeader>
          <TableBody>
            {channels.map((channel) => (
              <TableRow key={channel.id}>
                <TableCell>
                  <VStack gap={0}>
                    <Text weight="medium">{channel.name}</Text>
                    <Text type="supporting" color="secondary">
                      {channel.id}
                    </Text>
                  </VStack>
                </TableCell>
                <TableCell>
                  {/* A kind this admin doesn't know yet reads as custom. */}
                  <Badge
                    label={t(`settings.channel_kind_${knownKind(channel.kind) ?? "custom"}`)}
                  />
                </TableCell>
                <TableCell>
                  <Text color="secondary">
                    <DateTime value={channel.created_at} short />
                  </Text>
                </TableCell>
                <TableCell>
                  <HStack gap={1} hAlign="end">
                    <IconButton
                      label={t("settings.channel_rename", { name: channel.name })}
                      icon={<PencilSquareIcon />}
                      size="sm"
                      variant="ghost"
                      onClick={() => setEditing(channel)}
                    />
                    <IconButton
                      label={t("settings.channel_new_key_for", { name: channel.name })}
                      icon={<KeyIcon />}
                      size="sm"
                      variant="ghost"
                      tooltip={t("settings.channel_new_key")}
                      onClick={() => setKeyFor(channel)}
                    />
                  </HStack>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </VStack>
      {editing && (
        <ChannelDialog
          channel={editing === "new" ? undefined : editing}
          onClose={() => setEditing(undefined)}
        />
      )}
      {keyFor && <KeyDialog channel={keyFor} onClose={() => setKeyFor(undefined)} />}
    </Card>
  );
}

/** Adds a channel, or renames one (`channel`): its kind can't change. */
function ChannelDialog({ channel, onClose }: { channel?: Channel; onClose: () => void }) {
  const t = useT<"settings">();
  const [name, setName] = useState(channel?.name ?? "");
  const [kind, setKind] = useState<ChannelKind>("online_store");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const isComplete = name.trim() !== "" && name.trim() !== channel?.name;

  const submit = () => {
    if (!isComplete) return;
    startTransition(async () => {
      const result = channel
        ? await renameChannel(channel.id, name)
        : await createChannel(name, kind);
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={480} purpose="form">
      <DialogHeader
        title={
          channel
            ? t("settings.channel_dialog_title_rename", { name: channel.name })
            : t("settings.channel_dialog_title_add")
        }
        subtitle={channel ? undefined : t("settings.channel_dialog_subtitle")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={t("common.name")}
          description={t("settings.channel_name_description")}
          placeholder={t("settings.channel_name_placeholder")}
          value={name}
          onChange={setName}
          onEnter={submit}
        />
        {!channel && (
          <Selector
            label={t("settings.channel_kind_label")}
            description={t("settings.channel_kind_description")}
            options={KINDS.map((value) => ({
              value,
              label: t(`settings.channel_kind_${value}`),
              description: t(`settings.channel_kind_${value}_description`),
            }))}
            value={kind}
            onChange={(value) => {
              const picked = knownKind(value);
              if (picked) setKind(picked);
            }}
          />
        )}
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={channel ? t("common.save") : t("settings.channel_dialog_submit_add")}
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

/** Creates a publishable key for the channel and shows it. */
function KeyDialog({ channel, onClose }: { channel: Channel; onClose: () => void }) {
  const t = useT<"settings">();
  // The key's name is stored: English whatever the staff member's language.
  const [name, setName] = useState(`${channel.name} storefront`);
  const [token, setToken] = useState<string>();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const submit = () => {
    if (!name.trim()) return;
    startTransition(async () => {
      const result = await createStorefrontKey(channel.id, name);
      if (result.error) setError(result.error);
      else setToken(result.token);
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560} purpose="form">
      <DialogHeader
        title={t("settings.channel_key_title", { name: channel.name })}
        subtitle={
          token ? t("settings.channel_key_subtitle_created") : t("settings.channel_key_subtitle")
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {token ? (
          <CodeBlock code={token} />
        ) : (
          <TextInput
            label={t("settings.channel_key_name_label")}
            value={name}
            onChange={setName}
            onEnter={submit}
          />
        )}
        <HStack gap={2} hAlign="end">
          {token ? (
            <Button label={t("common.done")} variant="primary" onClick={onClose} />
          ) : (
            <>
              <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
              <Button
                label={t("settings.channel_key_create")}
                variant="primary"
                isLoading={isPending}
                isDisabled={!name.trim()}
                onClick={submit}
              />
            </>
          )}
        </HStack>
      </VStack>
    </Dialog>
  );
}
