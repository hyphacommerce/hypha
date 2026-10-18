import type { Metadata } from "next";

import { LinkProblem } from "../login/link-problem";
import { InviteForm } from "./invite-form";

// The token is in the URL: never pass it on to another site as a referrer.
export const metadata: Metadata = { title: "Join the team · Hypha admin", referrer: "no-referrer" };

/** Where an invite email's link lands (`{admin_url}/invite?token=…`, design note 0008). */
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
  return <InviteForm token={token} />;
}
