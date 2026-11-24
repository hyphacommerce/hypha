import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { getLanguage, offeredLanguages } from "@/lib/language";

import { LanguageCard, PasswordCard, ProfileCard, SessionsCard } from "./account-cards";
import { TwoFactorCard } from "./two-factor";

/**
 * The signed-in staff member's own account: profile, language, password, two-factor,
 * sessions. Everything but the sessions is known from the session itself and renders at
 * once.
 */
export default async function AccountPage() {
  const [me, t, language] = await Promise.all([requireStaff(), getT(), getLanguage()]);
  const languages = offeredLanguages();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={720}>
        <PageHeader title={t("common.your_account")} description={t("account.page_description")} />
        <ProfileCard name={me.user.name} email={me.user.email} role={me.role.name} />
        {/* With one language there's nothing to choose. */}
        {languages.length > 1 && <LanguageCard languages={languages} current={language} />}
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
