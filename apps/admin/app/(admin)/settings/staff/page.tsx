import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";

import { Staff } from "./staff";

export default async function StaffPage() {
  const client = await sessionClient();
  const [me, users, invites, roles] = await Promise.all([
    requireStaff(),
    client.GET("/admin/users").then(loaded),
    client.GET("/admin/invites").then(loaded),
    client.GET("/admin/roles").then(loaded),
  ]);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader title="Staff" description="Who can sign in, and what their roles allow." />
        <Staff
          users={users.items}
          invites={invites.items}
          roles={roles.items}
          canInvite={me.scopes.includes("users:write")}
        />
      </VStack>
    </Section>
  );
}
