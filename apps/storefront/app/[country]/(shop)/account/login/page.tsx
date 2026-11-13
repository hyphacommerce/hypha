import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { SignInForm } from "@/components/account/sign-in-form";
import { sessionToken } from "@/lib/account";
import { countryOf, loadStore } from "@/lib/api";
import { pageT } from "@/lib/i18n";
import { link } from "@/lib/site";

export const dynamic = "force-dynamic";

export async function generateMetadata({
  params,
}: {
  params: Promise<{ country: string }>;
}): Promise<Metadata> {
  const t = await pageT((await params).country);
  return { title: t("account.login_meta_title") };
}

/** A query value, if it was given once. */
function one(value: string | string[] | undefined): string {
  return typeof value === "string" ? value : "";
}

/** The sign-in code email's link lands here with the email and code filled in. */
export default async function LoginPage({
  params,
  searchParams,
}: {
  params: Promise<{ country: string }>;
  searchParams: Promise<{ email?: string | string[]; code?: string | string[] }>;
}) {
  const country = await countryOf((await params).country);
  if (await sessionToken()) redirect(link(country.country, "/account"));
  const q = await searchParams;
  return (
    <SignInForm
      country={country.country}
      storeName={(await loadStore()).name}
      initialEmail={one(q.email)}
      initialCode={one(q.code)}
    />
  );
}
