import { LoginForm } from "./login-form";

/** Signing in; after a password reset, with a note that the new password is set. */
export default async function LoginPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const { reset } = await searchParams;
  return (
    <LoginForm notice={reset === "done" ? "Your password is set. Sign in with it." : undefined} />
  );
}
