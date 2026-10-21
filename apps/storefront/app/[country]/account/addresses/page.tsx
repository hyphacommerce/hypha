import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { AccountShell } from "@/components/account/account-shell";
import { AddressBook } from "@/components/account/forms";
import { loadAccount } from "@/lib/account";
import { countryOf } from "@/lib/api";
import { link } from "@/lib/site";

export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Addresses" };

export default async function AddressesPage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const to = (path: string) => link(code, path);
  const account = await loadAccount();
  if (!account) redirect(to("/account"));
  return (
    <AccountShell title="Addresses" current="/account/addresses" link={to}>
      <AddressBook
        country={code}
        countryName={country.name}
        addresses={account.addresses.map((a) => ({
          id: a.id,
          lines: [
            [a.first_name, a.last_name].filter(Boolean).join(" "),
            a.company,
            a.line1,
            a.line2,
            [a.postal_code, a.city].filter(Boolean).join(" "),
            a.province,
            a.country,
            a.phone,
          ].filter((l): l is string => Boolean(l)),
          isShipping: account.default_shipping_address === a.id,
          isBilling: account.default_billing_address === a.id,
          fields: {
            first_name: a.first_name ?? "",
            last_name: a.last_name,
            company: a.company ?? "",
            line1: a.line1,
            line2: a.line2 ?? "",
            postal_code: a.postal_code ?? "",
            city: a.city,
            province: a.province ?? "",
            phone: a.phone ?? "",
            default_shipping: account.default_shipping_address === a.id,
            default_billing: account.default_billing_address === a.id,
          },
        }))}
      />
    </AccountShell>
  );
}
