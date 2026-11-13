import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { AccountShell } from "@/components/account/account-shell";
import { ProfileForm } from "@/components/account/forms";
import { loadAccount } from "@/lib/account";
import { countryOf } from "@/lib/api";
import { pageT } from "@/lib/i18n";
import { link } from "@/lib/site";

export const dynamic = "force-dynamic";

export async function generateMetadata({
  params,
}: {
  params: Promise<{ country: string }>;
}): Promise<Metadata> {
  const t = await pageT((await params).country);
  return { title: t("account.profile_meta_title") };
}

export default async function ProfilePage({ params }: { params: Promise<{ country: string }> }) {
  const folder = (await params).country;
  const [country, t] = await Promise.all([countryOf(folder), pageT(folder)]);
  const code = country.country;
  const to = (path: string) => link(code, path);
  const account = await loadAccount();
  if (!account) redirect(to("/account"));
  return (
    <AccountShell t={t} title={t("account.profile")} current="/account/profile" link={to}>
      <VStack gap={5}>
        <VStack gap={1}>
          <Text weight="bold">{t("account.email")}</Text>
          <Text>{account.email}</Text>
          <Text type="supporting" color="secondary">
            {t("account.email_note")}
          </Text>
        </VStack>
        <ProfileForm
          country={code}
          initial={{
            first_name: account.first_name ?? "",
            last_name: account.last_name ?? "",
            phone: account.phone ?? "",
          }}
        />
      </VStack>
    </AccountShell>
  );
}
