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
import type { AdminSchemas } from "@hypha/sdk/admin";
import { KeyIcon, PencilSquareIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";



import { type ChannelKind, createChannel, createStorefrontKey, renameChannel } from "./actions";
import { DateTime } from "@/components/date-time";

type Channel = AdminSchemas["ChannelResponse"];

const KINDS: { value: ChannelKind; label: string; description: string }[] = [
  { value: "online_store", label: "Online store", description: "A storefront you run." },
  { value: "agentic", label: "AI agents", description: "Shopping agents, through UCP." },
  { value: "custom", label: "Custom", description: "Anywhere else: a marketplace, a kiosk." },
];

/** A kind's label; one this admin doesn't know yet reads as custom. */
function kindLabel(kind: string): string {
  return KINDS.find((k) => k.value === kind)?.label ?? "Custom";
}

export function ChannelsCard({ channels }: { channels: Channel[] }) {
  const [editing, setEditing] = useState<Channel | "new">();
  const [keyFor, setKeyFor] = useState<Channel>();

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Channels
            </Text>
            <Text type="supporting" color="secondary">
              A storefront sells through the channel its publishable key belongs to.
            </Text>
          </VStack>
          <Button label="Add channel" variant="secondary" onClick={() => setEditing("new")} />
        </HStack>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHeaderCell>Channel</TableHeaderCell>
              <TableHeaderCell>Kind</TableHeaderCell>
              <TableHeaderCell>Created</TableHeaderCell>
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
                  <Badge label={kindLabel(channel.kind)} />
                </TableCell>
                <TableCell>
                  <Text color="secondary"><DateTime value={channel.created_at} short /></Text>
                </TableCell>
                <TableCell>
                  <HStack gap={1} hAlign="end">
                    <IconButton
                      label={`Rename ${channel.name}`}
                      icon={<PencilSquareIcon />}
                      size="sm"
                      variant="ghost"
                      onClick={() => setEditing(channel)}
                    />
                    <IconButton
                      label={`New storefront key for ${channel.name}`}
                      icon={<KeyIcon />}
                      size="sm"
                      variant="ghost"
                      tooltip="New storefront key"
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
  const [name, setName] = useState(channel?.name ?? "");
  const [kind, setKind] = useState<ChannelKind>("online_store");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const isComplete = name.trim() !== "" && name.trim() !== channel?.name;

  const submit = () => {
    if (!isComplete) return;
    startTransition(async () => {
      const result = channel ? await renameChannel(channel.id, name) : await createChannel(name, kind);
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={480} purpose="form">
      <DialogHeader
        title={channel ? `Rename ${channel.name}` : "Add channel"}
        subtitle={channel ? undefined : "It sells nothing until you publish products to it."}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label="Name"
          description="Shown to staff. Unique, ignoring case."
          placeholder="Outlet"
          value={name}
          onChange={setName}
          onEnter={submit}
        />
        {!channel && (
          <Selector
            label="Kind"
            description="What the channel is for. Can't change later."
            options={KINDS}
            value={kind}
            onChange={(value) => setKind(value as ChannelKind)}
          />
        )}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={channel ? "Save" : "Add channel"}
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
        title={`Storefront key for ${channel.name}`}
        subtitle={
          token
            ? "Copy it now: it isn't shown again. It's publishable, so it can sit in a storefront's code."
            : "A publishable key: storefronts read and sell what this channel publishes with it."
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {token ? (
          <CodeBlock code={token} />
        ) : (
          <TextInput label="Key name" value={name} onChange={setName} onEnter={submit} />
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
