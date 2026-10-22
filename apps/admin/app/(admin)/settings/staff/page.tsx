import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { TableSkeleton } from "@/components/skeletons";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { SETTINGS_CRUMB } from "@/lib/navigation";

import { Staff } from "./staff";

/** Settings → Staff: the header renders at once; the team, invites and roles stream in. */
export default function StaffPage() {
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Staff and roles"
          description="Who can sign in, and what their roles allow."
        />
        <Suspense
          fallback={
            <VStack gap={8}>
              <TableSkeleton rows={4} />
              <TableSkeleton rows={2} />
              <TableSkeleton rows={3} />
            </VStack>
          }
        >
          <Team />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Team() {
  const client = await sessionClient();
  const [me, users, invites, roles] = await Promise.all([
    requireStaff(),
    client.GET("/admin/users"),
    client.GET("/admin/invites"),
    client.GET("/admin/roles"),
  ]);
  // Users, invites and roles all need users:read: without it, say so instead of failing.
  const canRead = users.response.status !== 403;
  // From the server, so the browser renders the same expired badges it sent. A server
  // component renders once per request, so reading the clock here is fine.
  // oxlint-disable-next-line react/purity
  const now = Date.now();

  if (!canRead) {
    return (
      <Banner
        status="info"
        title="Your role can't see the team"
        description="Ask the store owner for the users:read scope. Your own account is under Your account."
        endContent={<Button label="Your account" variant="secondary" href="/account" />}
      />
    );
  }
  return (
    <Staff
      users={loaded(users).items}
      invites={loaded(invites).items}
      roles={loaded(roles).items}
      canInvite={me.scopes.includes("users:write")}
      myId={me.user.id}
      myScopes={me.scopes}
      now={now}
    />
  );
}
