import type { Metadata } from "next";

import { describe, publicClient } from "@/lib/hypha";

import { LinkProblem } from "../login/link-problem";
import { InviteForm } from "./invite-form";

// The token is in the URL: never pass it on to another site as a referrer.
export const metadata: Metadata = { title: "Join the team · Hypha admin", referrer: "no-referrer" };

/**
 * Where an invite email's link lands (`{admin_url}/invite?token=…`, design note 0008):
 * the invite looked up first, so the page says which store, as whom and from whom, and a
 * link that no longer works says so before anyone types a password.
 */
export default async function InvitePage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const { token } = await searchParams;
  if (typeof token !== "string" || !token) {
    return (
      <LinkProblem
        title="This invite link is incomplete"
        subtitle="Open the link from your invite email again, or ask the person who invited you to resend it."
      />
    );
  }
  const client = await publicClient();
  const { data, error } = await client.POST("/admin/invites/lookup", { body: { token } });
  if (error) {
    return error.code === "invalid_token" ? (
      <LinkProblem
        title="This invite link no longer works"
        subtitle="It expired, was withdrawn, or was already used. Ask the person who invited you to send a new one. If you already joined, sign in."
      />
    ) : (
      <LinkProblem title="The invite can't be opened right now" subtitle={describe(error)} />
    );
  }
  return <InviteForm token={token} invite={data} />;
}
