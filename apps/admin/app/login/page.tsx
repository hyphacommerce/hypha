import { getT } from "@/lib/i18n";

import { LoginForm } from "./login-form";

/** Signing in; after a password reset, with a note that the new password is set. */
export default async function LoginPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ reset }, t] = await Promise.all([searchParams, getT()]);
  return <LoginForm notice={reset === "done" ? t("auth.login_password_set") : undefined} />;
}
