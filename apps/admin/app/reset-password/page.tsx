import type { Metadata } from "next";

import { getT } from "@/lib/i18n";

import { LinkProblem } from "../login/link-problem";
import { ResetForm } from "./reset-form";

// The token is in the URL: never pass it on to another site as a referrer.
export async function generateMetadata(): Promise<Metadata> {
  const t = await getT();
  return { title: t("auth.reset_page_title"), referrer: "no-referrer" };
}

/** Where a reset email's link lands (`{admin_url}/reset-password?token=…`). */
export default async function ResetPasswordPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const { token } = await searchParams;
  if (typeof token !== "string" || !token) {
    const t = await getT();
    return (
      <LinkProblem
        title={t("auth.reset_incomplete_title")}
        subtitle={t("auth.reset_incomplete_subtitle")}
        action={{ label: t("auth.reset_incomplete_action"), href: "/forgot-password" }}
      />
    );
  }
  return <ResetForm token={token} />;
}
