import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { TableSkeleton } from "@/components/skeletons";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";

import { Staff } from "./staff";

/** Settings → Staff: the header renders at once; the team, invites and roles stream in. */
export default async function StaffPage() {
  const t = await getT();
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          back={settingsCrumb(t)}
          title={t("staff.page_title")}
          description={t("staff.page_description")}
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
  const [t, me, users, invites, roles] = await Promise.all([
    getT(),
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
        title={t("staff.no_access_title")}
        description={t("staff.no_access_description", { scope: "users:read" })}
        endContent={<Button label={t("common.your_account")} variant="secondary" href="/account" />}
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
