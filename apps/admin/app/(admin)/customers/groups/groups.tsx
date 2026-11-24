"use client";

// The customer groups table: name, handle, description, with dialogs to add, change and
// delete a group. A group's customers are the customers list filtered by it.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import { PencilSquareIcon, PlusIcon, TrashIcon, UserGroupIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useRouter } from "next/navigation";
import { useRef, useState, useTransition } from "react";

import { toHandle } from "@/lib/categories";
import { useT } from "@/lib/use-t";

import { createGroup, deleteGroup, updateGroup } from "./actions";

export type Group = AdminSchemas["CustomerGroupResponse"];

export function GroupsCard({ groups, canWrite }: { groups: Group[]; canWrite: boolean }) {
  const t = useT<"customers">();
  const [editing, setEditing] = useState<Group | "new">();
  const [deleting, setDeleting] = useState<Group>();
  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("customers.groups_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("customers.groups_description")}
            </Text>
          </VStack>
          {canWrite && (
            <Button
              label={t("customers.groups_add")}
              variant="primary"
              icon={<PlusIcon />}
              onClick={() => setEditing("new")}
            />
          )}
        </HStack>
        {groups.length === 0 ? (
          <EmptyState
            icon={<UserGroupIcon width={32} />}
            title={t("customers.groups_empty_title")}
            description={t("customers.groups_empty_description")}
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("customers.group")}</TableHeaderCell>
                <TableHeaderCell>{t("common.description")}</TableHeaderCell>
                <TableHeaderCell>
                  <span className="visually-hidden">{t("common.actions")}</span>
                </TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {groups.map((group) => (
                <TableRow key={group.id}>
                  <TableCell>
                    <VStack gap={0.5}>
                      <Link href={`/customers?group=${group.id}`} weight="medium">
                        {group.name}
                      </Link>
                      <Text type="supporting" color="secondary">
                        {group.handle}
                      </Text>
                    </VStack>
                  </TableCell>
                  <TableCell>
                    <Text color={group.description ? "primary" : "secondary"}>
                      {group.description ?? t("common.empty_value")}
                    </Text>
                  </TableCell>
                  <TableCell>
                    {canWrite && (
                      <HStack gap={1} hAlign="end">
                        <IconButton
                          label={t("common.edit_named", { name: group.name })}
                          icon={<PencilSquareIcon />}
                          size="sm"
                          variant="ghost"
                          onClick={() => setEditing(group)}
                        />
                        <IconButton
                          label={t("common.delete_named_label", { name: group.name })}
                          icon={<TrashIcon />}
                          size="sm"
                          variant="ghost"
                          onClick={() => setDeleting(group)}
                        />
                      </HStack>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </VStack>
      {editing && (
        <GroupDialog
          group={editing === "new" ? undefined : editing}
          onClose={() => setEditing(undefined)}
        />
      )}
      {deleting && <DeleteGroupDialog group={deleting} onClose={() => setDeleting(undefined)} />}
    </Card>
  );
}

function GroupDialog({ group, onClose }: { group?: Group; onClose: () => void }) {
  const router = useRouter();
  const t = useT<"customers">();
  const [name, setName] = useState(group?.name ?? "");
  const [handle, setHandle] = useState(group?.handle ?? "");
  // Whether the handle was typed by hand: until then it follows the name.
  const handleTouched = useRef(group !== undefined);
  const [description, setDescription] = useState(group?.description ?? "");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const submit = () => {
    if (name.trim() === "" || handle.trim() === "") {
      setError(t("customers.groups_dialog_incomplete"));
      return;
    }
    startTransition(async () => {
      const body = {
        name: name.trim(),
        handle: handle.trim(),
        description: description.trim() === "" ? null : description.trim(),
      };
      const result = group ? await updateGroup(group.id, body) : await createGroup(body);
      if (result.error) {
        setError(result.error);
        return;
      }
      router.refresh();
      onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={480} purpose="form">
      <DialogHeader
        title={
          group
            ? t("customers.groups_dialog_edit_title", { name: group.name })
            : t("customers.groups_dialog_add_title")
        }
        subtitle={t("customers.groups_dialog_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={t("common.name")}
          isRequired
          value={name}
          onChange={(value) => {
            setName(value);
            if (!handleTouched.current) setHandle(toHandle(value));
          }}
        />
        <TextInput
          label={t("common.handle")}
          description={t("customers.groups_dialog_handle_description")}
          isRequired
          value={handle}
          onChange={(value) => {
            handleTouched.current = true;
            setHandle(value);
          }}
        />
        <TextArea
          label={t("common.description")}
          isOptional
          rows={3}
          maxLength={1000}
          value={description}
          onChange={setDescription}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={group ? t("common.save") : t("customers.groups_add")}
            variant="primary"
            isLoading={isPending}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

function DeleteGroupDialog({ group, onClose }: { group: Group; onClose: () => void }) {
  const router = useRouter();
  const t = useT<"customers">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(open) => !open && onClose()}
      title={t("common.delete_named", { name: group.name })}
      description={error ?? t("customers.groups_delete_description")}
      actionLabel={t("customers.groups_delete_action")}
      isActionLoading={isPending}
      onAction={() =>
        startTransition(async () => {
          const result = await deleteGroup(group.id);
          if (result.error) {
            setError(result.error);
            return;
          }
          router.refresh();
          onClose();
        })
      }
    />
  );
}
