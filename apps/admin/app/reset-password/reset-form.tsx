"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type FormEvent, startTransition, useActionState, useState } from "react";

import { NewPasswordFields, fieldText, newPasswordProblem } from "@/components/new-password";

import { confirmPasswordReset } from "../login/actions";
import { AuthCard } from "../login/auth-card";

/** Choosing a new password with a reset link's token; then sign in with it. */
export function ResetForm({ token }: { token: string }) {
  const [problem, setProblem] = useState<string>();
  const [state, confirm, isPending] = useActionState(confirmPasswordReset, {});

  const submit = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const fields = new FormData(e.currentTarget);
    const password = fieldText(fields, "new-password");
    const found = newPasswordProblem(password, fieldText(fields, "repeat-password"));
    setProblem(found);
    if (!found) startTransition(() => confirm({ token, password }));
  };

  return (
    <AuthCard
      title="Choose a new password"
      subtitle="Every device signed in to your account is signed out. Two-factor authentication stays as it is."
    >
      {(problem ?? state.error) && <Banner status="error" title={problem ?? state.error ?? ""} />}
      <form onSubmit={submit} noValidate>
        <VStack gap={4}>
          <NewPasswordFields />
          <Button
            label="Set the password"
            type="submit"
            variant="primary"
            size="lg"
            isLoading={isPending}
          />
        </VStack>
      </form>
      <Link href="/forgot-password" size="sm" color="secondary" type="supporting">
        Need a new link?
      </Link>
    </AuthCard>
  );
}
