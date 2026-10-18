import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";

import { PasswordCard, ProfileCard, SessionsCard } from "./account-cards";
import { TwoFactorCard } from "./two-factor";

/** The signed-in staff member's own account: profile, password, two-factor, sessions. */
export default async function AccountPage() {
  const client = await sessionClient();
  const [me, sessions] = await Promise.all([
    requireStaff(),
    client.GET("/admin/me/sessions").then(loaded),
  ]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={720}>
        <PageHeader title="Your account" description="How you sign in, and what your team sees." />
        <ProfileCard name={me.user.name} email={me.user.email} role={me.role.name} />
        <TwoFactorCard isEnabled={me.mfa_enabled} />
        <PasswordCard />
        <SessionsCard sessions={sessions.items} />
      </VStack>
    </Section>
  );
}
