import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { AccountShell } from "@/components/account/account-shell";
import { MarketingChoices } from "@/components/account/forms";
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
  return { title: t("account.marketing_meta_title") };
}

export default async function MarketingPage({ params }: { params: Promise<{ country: string }> }) {
  const folder = (await params).country;
  const [, t] = await Promise.all([countryOf(folder), pageT(folder)]);
  const to = (path: string) => link(folder, path);
  const account = await loadAccount();
  if (!account) redirect(to("/account"));
  return (
    <AccountShell t={t} title={t("account.marketing")} current="/account/marketing" link={to}>
      <MarketingChoices
        email={account.email_marketing.state === "subscribed"}
        sms={account.sms_marketing.state === "subscribed"}
        hasPhone={Boolean(account.phone)}
      />
    </AccountShell>
  );
}
