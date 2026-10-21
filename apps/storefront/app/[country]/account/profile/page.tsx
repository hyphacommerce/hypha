import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { AccountShell } from "@/components/account/account-shell";
import { ProfileForm } from "@/components/account/forms";
import { loadAccount } from "@/lib/account";
import { countryOf } from "@/lib/api";
import { link } from "@/lib/site";

export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Profile" };

export default async function ProfilePage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const to = (path: string) => link(code, path);
  const account = await loadAccount();
  if (!account) redirect(to("/account"));
  return (
    <AccountShell title="Profile" current="/account/profile" link={to}>
      <VStack gap={5}>
        <VStack gap={1}>
          <Text weight="bold">Email</Text>
          <Text>{account.email}</Text>
          <Text type="supporting" color="secondary">
            You sign in with this address, so it can&apos;t be changed here.
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
