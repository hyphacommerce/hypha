"use client";

// Staff, their pending invites and the roles (design note 0024, AU6): whole lists, so
// they sort and filter here.

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { type TableColumn, pixel } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { EnvelopeIcon, ShieldCheckIcon, UsersIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { ClientResourceTable, byText } from "@/components/resource-table/resource-table";


import { inviteStaff } from "./actions";

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
}: {
  users: User[];
  invites: Invite[];
  roles: Role[];
  canInvite: boolean;
}) {
  const [inviting, setInviting] = useState<{ email: string; role?: string }>();
  const roleName = new Map(roles.map((r) => [r.id, r.name]));
  const memberCount = new Map<string, number>();
  for (const u of users) memberCount.set(u.role, (memberCount.get(u.role) ?? 0) + 1);
  const now = Date.now();

  const userColumns: TableColumn<User>[] = [
    { key: "name", header: "Name" },
    { key: "email", header: "Email" },
    { key: "role", header: "Role", renderCell: (u) => roleName.get(u.role) ?? u.role },
    {
      key: "status",
      header: "Status",
      width: pixel(110),
      renderCell: (u) => (
        <Badge label={u.status === "active" ? "Active" : "Disabled"} variant={u.status === "active" ? "success" : "neutral"} />
      ),
    },
    { key: "created_at", header: "Since", width: pixel(190), renderCell: (u) => <DateTime value={u.created_at} short /> },
  ];
  const inviteColumns: TableColumn<Invite>[] = [
    { key: "email", header: "Email" },
    { key: "role", header: "Role", renderCell: (i) => roleName.get(i.role) ?? i.role },
    {
      key: "expires_at",
      header: "Expires",
      width: pixel(230),
      renderCell: (i) =>
        Date.parse(i.expires_at) < now ? (
          <Badge label="Expired" variant="warning" />
        ) : (
          <DateTime value={i.expires_at} short />
        ),
    },
    {
      key: "actions",
      header: "",
      width: pixel(110),
      align: "end",
      renderCell: (i) =>
        canInvite && (
          <Button
            label="Resend"
            size="sm"
            variant="ghost"
            onClick={() => setInviting({ email: i.email, role: i.role })}
          />
        ),
    },
  ];
  const roleColumns: TableColumn<Role>[] = [
    {
      key: "name",
      header: "Role",
      renderCell: (r) => (
        <HStack gap={2} vAlign="center">
          <Text weight="medium">{r.name}</Text>
          {r.owner && <Badge label="Every scope" variant="info" />}
        </HStack>
      ),
    },
    {
      key: "scopes",
      header: "Scopes",
      renderCell: (r) => (r.owner ? "All, including future ones" : r.scopes.join(", ") || "None"),
    },
    {
      key: "members",
      header: "Members",
      width: pixel(100),
      align: "end",
      renderCell: (r) => memberCount.get(r.id) ?? 0,
    },
  ];

  return (
    <VStack gap={8}>
      <VStack gap={4}>
        <HStack hAlign="between" vAlign="start" gap={3}>
          <Heading title="Staff" description="Everyone who can sign in to this admin." />
          {canInvite && (
            <Button label="Invite staff" variant="primary" onClick={() => setInviting({ email: "" })} />
          )}
        </HStack>
        <ClientResourceTable<User>
          rows={users}
          columns={userColumns}
          idKey="id"
          searchText={(u) => `${u.name} ${u.email}`}
          searchLabel="Search staff"
          compare={{
            name: byText((u: User) => u.name),
            email: byText((u: User) => u.email),
            created_at: byText((u: User) => u.created_at),
          }}
          empty={{ icon: <UsersIcon width={32} />, title: "No staff yet" }}
        />
      </VStack>

      <VStack gap={4}>
        <Heading
          title="Pending invites"
          description="An invite lasts a few days; resending it makes a new link and ends the old one."
        />
        <ClientResourceTable<Invite>
          rows={invites}
          columns={inviteColumns}
          idKey="id"
          compare={{
            email: byText((i: Invite) => i.email),
            expires_at: byText((i: Invite) => i.expires_at),
          }}
          empty={{ icon: <EnvelopeIcon width={32} />, title: "No pending invites" }}
        />
      </VStack>

      <VStack gap={4}>
        <Heading
          title="Roles"
          description="What each role may do. Nobody grants a scope they don't hold themselves."
        />
        <ClientResourceTable<Role>
          rows={roles}
          columns={roleColumns}
          idKey="id"
          compare={{ name: byText((r: Role) => r.name) }}
          empty={{ icon: <ShieldCheckIcon width={32} />, title: "No roles" }}
        />
      </VStack>

      {inviting && (
        <InviteDialog
          roles={roles}
          initial={inviting}
          onClose={() => setInviting(undefined)}
        />
      )}
    </VStack>
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
        title={initial.email ? `Resend the invite to ${initial.email}` : "Invite staff"}
        subtitle="They get an email with a link to set their name and password."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {sent ? (
          <Banner status="success" title={`Invite sent to ${email.trim()}.`} />
        ) : (
          <>
            <TextInput
              label="Email"
              type="email"
              placeholder="name@company.com"
              value={email}
              onChange={setEmail}
              onEnter={submit}
              isReadOnly={Boolean(initial.email)}
            />
            <Selector
              label="Role"
              description="What they may do once they join."
              options={roles.map((r) => ({ value: r.id, label: r.name }))}
              value={role}
              onChange={setRole}
            />
          </>
        )}
        <HStack gap={2} hAlign="end">
          {sent ? (
            <Button label="Done" variant="primary" onClick={onClose} />
          ) : (
            <>
              <Button label="Cancel" variant="ghost" onClick={onClose} />
              <Button
                label={initial.email ? "Resend invite" : "Send invite"}
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
