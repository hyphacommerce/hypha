"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type FormEvent, startTransition, useActionState, useState } from "react";

import { NewPasswordFields, fieldText, newPasswordProblem } from "@/components/new-password";
import { useT } from "@/lib/use-t";

import { confirmPasswordReset } from "../login/actions";
import { AuthCard } from "../login/auth-card";

/** Choosing a new password with a reset link's token; then sign in with it. */
export function ResetForm({ token }: { token: string }) {
  const t = useT<"auth">();
  const [problem, setProblem] = useState<string>();
  const [state, confirm, isPending] = useActionState(confirmPasswordReset, {});

  const submit = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const fields = new FormData(e.currentTarget);
    const password = fieldText(fields, "new-password");
    const found = newPasswordProblem(password, fieldText(fields, "repeat-password"), t);
    setProblem(found);
    if (!found) startTransition(() => confirm({ token, password }));
  };

  return (
    <AuthCard title={t("auth.reset_title")} subtitle={t("auth.reset_subtitle")}>
      {(problem ?? state.error) && <Banner status="error" title={problem ?? state.error ?? ""} />}
      <form onSubmit={submit} noValidate>
        <VStack gap={4}>
          <NewPasswordFields />
          <Button
            label={t("auth.reset_submit")}
            type="submit"
            variant="primary"
            size="lg"
            isLoading={isPending}
          />
        </VStack>
      </form>
      <Link href="/forgot-password" size="sm" color="secondary" type="supporting">
        {t("auth.reset_new_link")}
      </Link>
    </AuthCard>
  );
}
