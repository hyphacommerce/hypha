import type { Metadata } from "next";

import { LinkProblem } from "../login/link-problem";
import { ResetForm } from "./reset-form";

// The token is in the URL: never pass it on to another site as a referrer.
export const metadata: Metadata = {
  title: "Choose a new password · Hypha admin",
  referrer: "no-referrer",
};

/** Where a reset email's link lands (`{admin_url}/reset-password?token=…`). */
export default async function ResetPasswordPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const { token } = await searchParams;
  if (typeof token !== "string" || !token) {
    return (
      <LinkProblem
        title="This reset link is incomplete"
        subtitle="Open the link from the email again, or ask for a new one."
        action={{ label: "Ask for a new link", href: "/forgot-password" }}
      />
    );
  }
  return <ResetForm token={token} />;
}
