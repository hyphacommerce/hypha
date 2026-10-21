import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { AccountShell } from "@/components/account/account-shell";
import { MarketingChoices } from "@/components/account/forms";
import { loadAccount } from "@/lib/account";
import { countryOf } from "@/lib/api";
import { link } from "@/lib/site";

export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Marketing" };

export default async function MarketingPage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const to = (path: string) => link(code, path);
  const account = await loadAccount();
  if (!account) redirect(to("/account"));
  return (
    <AccountShell title="Marketing" current="/account/marketing" link={to}>
      <MarketingChoices
        country={code}
        email={account.email_marketing.state === "subscribed"}
        sms={account.sms_marketing.state === "subscribed"}
        hasPhone={Boolean(account.phone)}
      />
    </AccountShell>
  );
}
