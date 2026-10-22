import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";

import { PasswordCard, ProfileCard, SessionsCard } from "./account-cards";
import { TwoFactorCard } from "./two-factor";

/**
 * The signed-in staff member's own account: profile, password, two-factor, sessions.
 * Everything but the sessions is known from the session itself and renders at once.
 */
export default async function AccountPage() {
  const me = await requireStaff();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={720}>
        <PageHeader title="Your account" description="How you sign in, and what your team sees." />
        <ProfileCard name={me.user.name} email={me.user.email} role={me.role.name} />
        <TwoFactorCard isEnabled={me.mfa_enabled} />
        <PasswordCard />
        <Suspense fallback={<CardSkeleton lines={3} />}>
          <Sessions />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Sessions() {
  const client = await sessionClient();
  const sessions = await client.GET("/admin/me/sessions").then(loaded);
  return <SessionsCard sessions={sessions.items} />;
}
