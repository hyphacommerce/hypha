"use client";

// Staff, their pending invites and the roles: whole lists, so they sort and filter
// here.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Selector } from "@astryxdesign/core/Selector";
import { type TableColumn, pixel } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { EnvelopeIcon, ShieldCheckIcon, UsersIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { ClientResourceTable, byText } from "@/components/resource-table/resource-table";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { inviteStaff, revokeInvite, saveRole, updateStaff } from "./actions";

type Scope = AdminSchemas["Scope"];
type User = AdminSchemas["UserResponse"];
type Invite = AdminSchemas["InviteResponse"];
type Role = AdminSchemas["RoleResponse"];

function Heading({ title, description }: { title: string; description: string }) {
  return (
    <VStack gap={1}>
      <Text type="display-3" as="h2">
        {title}
      </Text>
      <Text color="secondary">{description}</Text>
    </VStack>
  );
}

export function Staff({
  users,
  invites,
  roles,
  canInvite,
  myId,
  myScopes,
  now,
}: {
  users: User[];
  invites: Invite[];
  roles: Role[];
  /** Whether the signed-in member holds `users:write`: inviting, managing, roles. */
  canInvite: boolean;
  myId: string;
  /** What the signed-in member may give away to a role. */
  myScopes: Scope[];
  /** When the page was read (ms since the epoch), for which invites have expired. */
  now: number;
}) {
  const [inviting, setInviting] = useState<{ email: string; role?: string }>();
  const [managing, setManaging] = useState<User>();
  const [revoking, setRevoking] = useState<Invite>();
  const [editingRole, setEditingRole] = useState<Role | "new">();
  const t = useT<"staff">();
  const format = useFormat();
  const roleName = new Map(roles.map((r) => [r.id, r.name]));
  const memberCount = new Map<string, number>();
  for (const u of users) memberCount.set(u.role, (memberCount.get(u.role) ?? 0) + 1);

  const userColumns: TableColumn<User>[] = [
    { key: "name", header: t("common.name") },
    { key: "email", header: t("common.email") },
    { key: "role", header: t("common.role"), renderCell: (u) => roleName.get(u.role) ?? u.role },
    {
      key: "status",
      header: t("common.status"),
      width: pixel(110),
      renderCell: (u) => (
        <Badge
          label={u.status === "active" ? t("common.active") : t("common.disabled")}
          variant={u.status === "active" ? "success" : "neutral"}
        />
      ),
    },
    {
      key: "created_at",
      header: t("staff.staff_since"),
      width: pixel(190),
      renderCell: (u) => <DateTime value={u.created_at} short />,
    },
    {
      key: "actions",
      header: "",
      width: pixel(110),
      align: "end",
      renderCell: (u) =>
        u.id === myId ? (
          <Button label={t("staff.staff_you")} size="sm" variant="ghost" href="/account" />
        ) : (
          canInvite && (
            <Button
              label={t("staff.staff_manage")}
              size="sm"
              variant="ghost"
              onClick={() => setManaging(u)}
            />
          )
        ),
    },
  ];
  const inviteColumns: TableColumn<Invite>[] = [
    { key: "email", header: t("common.email") },
    { key: "role", header: t("common.role"), renderCell: (i) => roleName.get(i.role) ?? i.role },
    {
      key: "expires_at",
      header: t("staff.invites_expires"),
      width: pixel(230),
      renderCell: (i) =>
        Date.parse(i.expires_at) < now ? (
          <Badge label={t("staff.invites_expired")} variant="warning" />
        ) : (
          <DateTime value={i.expires_at} short />
        ),
    },
    {
      key: "actions",
      header: "",
      width: pixel(190),
      align: "end",
      renderCell: (i) =>
        canInvite && (
          <HStack gap={1} hAlign="end">
            <Button
              label={t("common.resend")}
              size="sm"
              variant="ghost"
              onClick={() => setInviting({ email: i.email, role: i.role })}
            />
            <Button
              label={t("common.revoke")}
              size="sm"
              variant="ghost"
              onClick={() => setRevoking(i)}
            />
          </HStack>
        ),
    },
  ];
  const roleColumns: TableColumn<Role>[] = [
    {
      key: "name",
      header: t("common.role"),
      renderCell: (r) => (
        <HStack gap={2} vAlign="center">
          <Text weight="medium">{r.name}</Text>
          {r.owner && <Badge label={t("staff.roles_every_scope")} variant="info" />}
        </HStack>
      ),
    },
    {
      key: "scopes",
      header: t("common.scopes"),
      renderCell: (r) =>
        r.owner
          ? t("staff.roles_scopes_all")
          : r.scopes.length > 0
            ? format.list(r.scopes, "unit")
            : t("common.none"),
    },
    {
      key: "members",
      header: t("staff.roles_members"),
      width: pixel(100),
      align: "end",
      renderCell: (r) => memberCount.get(r.id) ?? 0,
    },
    {
      key: "actions",
      header: "",
      width: pixel(90),
      align: "end",
      renderCell: (r) =>
        canInvite &&
        !r.owner && (
          <Button
            label={t("common.edit")}
            size="sm"
            variant="ghost"
            onClick={() => setEditingRole(r)}
          />
        ),
    },
  ];

  return (
    <VStack gap={8}>
      <VStack gap={4}>
        <HStack hAlign="between" vAlign="start" gap={3}>
          <Heading title={t("staff.staff_title")} description={t("staff.staff_description")} />
          {canInvite && (
            <Button
              label={t("staff.invite_staff")}
              variant="primary"
              onClick={() => setInviting({ email: "" })}
            />
          )}
        </HStack>
        <ClientResourceTable<User>
          rows={users}
          columns={userColumns}
          idKey="id"
          searchText={(u) => `${u.name} ${u.email}`}
          searchLabel={t("staff.staff_search")}
          compare={{
            name: byText((u: User) => u.name),
            email: byText((u: User) => u.email),
            created_at: byText((u: User) => u.created_at),
          }}
          empty={{ icon: <UsersIcon width={32} />, title: t("staff.staff_empty") }}
        />
      </VStack>

      <VStack gap={4}>
        <Heading title={t("staff.invites_title")} description={t("staff.invites_description")} />
        <ClientResourceTable<Invite>
          rows={invites}
          columns={inviteColumns}
          idKey="id"
          compare={{
            email: byText((i: Invite) => i.email),
            expires_at: byText((i: Invite) => i.expires_at),
          }}
          empty={{ icon: <EnvelopeIcon width={32} />, title: t("staff.invites_empty") }}
        />
      </VStack>

      <VStack gap={4}>
        <HStack hAlign="between" vAlign="start" gap={3}>
          <Heading title={t("staff.roles_title")} description={t("staff.roles_description")} />
          {canInvite && (
            <Button
              label={t("staff.roles_new")}
              variant="secondary"
              onClick={() => setEditingRole("new")}
            />
          )}
        </HStack>
        <ClientResourceTable<Role>
          rows={roles}
          columns={roleColumns}
          idKey="id"
          compare={{ name: byText((r: Role) => r.name) }}
          empty={{ icon: <ShieldCheckIcon width={32} />, title: t("staff.roles_empty") }}
        />
      </VStack>

      {inviting && (
        <InviteDialog roles={roles} initial={inviting} onClose={() => setInviting(undefined)} />
      )}
      {managing && (
        <ManageDialog user={managing} roles={roles} onClose={() => setManaging(undefined)} />
      )}
      {revoking && <RevokeInviteDialog invite={revoking} onClose={() => setRevoking(undefined)} />}
      {editingRole && (
        <RoleDialog
          role={editingRole === "new" ? undefined : editingRole}
          scopes={myScopes}
          onClose={() => setEditingRole(undefined)}
        />
      )}
    </VStack>
  );
}

/** Asks before withdrawing an invite; its link stops working at once. */
function RevokeInviteDialog({ invite, onClose }: { invite: Invite; onClose: () => void }) {
  const t = useT<"staff">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const revoke = () =>
    startTransition(async () => {
      const result = await revokeInvite(invite.id);
      if (result.error) setError(result.error);
      else onClose();
    });
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={t("staff.revoke_dialog_title", { email: invite.email })}
      description={error ?? t("staff.revoke_dialog_description")}
      actionLabel={t("staff.revoke_dialog_action")}
      isActionLoading={isPending}
      onAction={revoke}
    />
  );
}

/** A staff member's role, and whether they may sign in. */
function ManageDialog({
  user,
  roles,
  onClose,
}: {
  user: User;
  roles: Role[];
  onClose: () => void;
}) {
  const t = useT<"staff">();
  const [role, setRole] = useState(user.role);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const isActive = user.status === "active";
  const onOpenChange = (isOpen: boolean) => !isOpen && onClose();

  const apply = (change: Parameters<typeof updateStaff>[1]) =>
    startTransition(async () => {
      const result = await updateStaff(user.id, change);
      if (result.error) setError(result.error);
      else onClose();
    });

  return (
    <Dialog isOpen onOpenChange={onOpenChange} width={480} purpose="form">
      <DialogHeader title={user.name} subtitle={user.email} onOpenChange={onOpenChange} />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <Selector
          label={t("common.role")}
          options={roles.map((r) => ({ value: r.id, label: r.name }))}
          value={role}
          onChange={setRole}
        />
        <HStack gap={2} hAlign="between" wrap="wrap">
          <Button
            label={isActive ? t("common.disable") : t("common.enable")}
            variant={isActive ? "destructive" : "secondary"}
            isDisabled={isPending}
            onClick={() => apply({ status: isActive ? "disabled" : "active" })}
          />
          <HStack gap={2}>
            <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
            <Button
              label={t("staff.manage_save_role")}
              variant="primary"
              isLoading={isPending}
              isDisabled={role === user.role}
              onClick={() => apply({ role })}
            />
          </HStack>
        </HStack>
        <Text type="supporting" color="secondary">
          {t("staff.manage_disable_hint")}
        </Text>
      </VStack>
    </Dialog>
  );
}

/** A new role, or an existing one's name and scopes. */
function RoleDialog({
  role,
  scopes,
  onClose,
}: {
  role?: Role;
  scopes: Scope[];
  onClose: () => void;
}) {
  const t = useT<"staff">();
  const [name, setName] = useState(role?.name ?? "");
  const [chosen, setChosen] = useState<string[]>(role?.scopes ?? []);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const onOpenChange = (isOpen: boolean) => !isOpen && onClose();
  // Scopes a role already has stay listed, so editing never drops one silently.
  const options = [...new Set<string>([...scopes, ...(role?.scopes ?? [])])];
  const isComplete = name.trim() !== "" && chosen.length > 0;

  const save = () =>
    startTransition(async () => {
      const picked = options.filter((s): s is Scope => chosen.includes(s));
      const result = await saveRole(role?.id, { name: name.trim(), scopes: picked });
      if (result.error) setError(result.error);
      else onClose();
    });

  return (
    <Dialog isOpen onOpenChange={onOpenChange} width={520} purpose="form">
      <DialogHeader
        title={
          role
            ? t("staff.role_dialog_edit_title", { name: role.name })
            : t("staff.role_dialog_new_title")
        }
        subtitle={t("staff.role_dialog_subtitle")}
        onOpenChange={onOpenChange}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={t("common.name")}
          placeholder={t("staff.role_dialog_name_placeholder")}
          value={name}
          onChange={setName}
        />
        <MultiSelector
          label={t("common.scopes")}
          description={t("staff.role_dialog_scopes_description")}
          options={options.map((s) => ({ value: s, label: s }))}
          value={chosen}
          onChange={setChosen}
          hasSearch
          hasSelectAll
          triggerDisplay="count"
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={role ? t("staff.manage_save_role") : t("staff.role_dialog_create")}
            variant="primary"
            isLoading={isPending}
            isDisabled={!isComplete}
            onClick={save}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

function InviteDialog({
  roles,
  initial,
  onClose,
}: {
  roles: Role[];
  initial: { email: string; role?: string };
  onClose: () => void;
}) {
  const t = useT<"staff">();
  const [email, setEmail] = useState(initial.email);
  const [role, setRole] = useState(initial.role ?? roles.find((r) => !r.owner)?.id ?? roles[0]?.id);
  const [error, setError] = useState<string>();
  const [sent, setSent] = useState(false);
  const [isPending, startTransition] = useTransition();
  const isComplete = email.trim() !== "" && Boolean(role);

  const submit = () => {
    if (!isComplete || !role) return;
    startTransition(async () => {
      const result = await inviteStaff(email.trim(), role);
      if (result.error) setError(result.error);
      else setSent(true);
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={480} purpose="form">
      <DialogHeader
        title={
          initial.email
            ? t("staff.invite_dialog_resend_title", { email: initial.email })
            : t("staff.invite_dialog_title")
        }
        subtitle={t("staff.invite_dialog_subtitle")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {sent ? (
          <Banner status="success" title={t("staff.invite_dialog_sent", { email: email.trim() })} />
        ) : (
          <>
            <TextInput
              label={t("common.email")}
              type="email"
              placeholder={t("staff.invite_dialog_email_placeholder")}
              value={email}
              onChange={setEmail}
              onEnter={submit}
              isReadOnly={Boolean(initial.email)}
            />
            <Selector
              label={t("common.role")}
              description={t("staff.invite_dialog_role_description")}
              options={roles.map((r) => ({ value: r.id, label: r.name }))}
              value={role}
              onChange={setRole}
            />
          </>
        )}
        <HStack gap={2} hAlign="end">
          {sent ? (
            <Button label={t("common.done")} variant="primary" onClick={onClose} />
          ) : (
            <>
              <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
              <Button
                label={
                  initial.email ? t("staff.invite_dialog_resend") : t("staff.invite_dialog_send")
                }
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
