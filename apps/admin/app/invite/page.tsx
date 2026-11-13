import type { Metadata } from "next";

import { describe, publicClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";

import { LinkProblem } from "../login/link-problem";
import { InviteForm } from "./invite-form";

// The token is in the URL: never pass it on to another site as a referrer.
export async function generateMetadata(): Promise<Metadata> {
  const t = await getT();
  return { title: t("auth.invite_page_title"), referrer: "no-referrer" };
}

/**
 * Where an invite email's link lands (`{admin_url}/invite?token=…`): the invite looked
 * up first, so the page says which store, as whom and from whom, and a link that no
 * longer works says so before anyone types a password.
 */
export default async function InvitePage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ token }, t] = await Promise.all([searchParams, getT()]);
  if (typeof token !== "string" || !token) {
    return (
      <LinkProblem
        title={t("auth.invite_incomplete_title")}
        subtitle={t("auth.invite_incomplete_subtitle")}
      />
    );
  }
  const client = await publicClient();
  const { data, error } = await client.POST("/admin/invites/lookup", { body: { token } });
  if (error) {
    return error.code === "invalid_token" ? (
      <LinkProblem
        title={t("auth.invite_invalid_title")}
        subtitle={t("auth.invite_invalid_subtitle")}
      />
    ) : (
      <LinkProblem title={t("auth.invite_unavailable_title")} subtitle={await describe(error)} />
    );
  }
  return <InviteForm token={token} invite={data} />;
}
