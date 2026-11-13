"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type FormEvent, startTransition, useActionState, useState } from "react";

import { fieldText } from "@/components/new-password";
import { useT } from "@/lib/use-t";

import { requestPasswordReset } from "../login/actions";
import { AuthCard } from "../login/auth-card";

/** Asks for a reset link by email. */
export function ForgotForm() {
  const t = useT<"auth">();
  const [email, setEmail] = useState("");
  const [isIncomplete, setIsIncomplete] = useState(false);
  const [state, request, isPending] = useActionState(requestPasswordReset, {});

  const submit = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const submitted = fieldText(new FormData(e.currentTarget), "email").trim();
    setEmail(submitted);
    setIsIncomplete(!submitted);
    if (submitted) startTransition(() => request(submitted));
  };

  if (state.sentTo) {
    return (
      <AuthCard
        title={t("auth.forgot_sent_title")}
        subtitle={t("auth.forgot_sent_subtitle", { email: state.sentTo })}
      >
        <Link href="/login" size="sm" color="secondary" type="supporting">
          {t("auth.back_to_sign_in")}
        </Link>
      </AuthCard>
    );
  }

  return (
    <AuthCard title={t("auth.forgot_title")} subtitle={t("auth.forgot_subtitle")}>
      {isIncomplete ? (
        <Banner status="error" title={t("auth.forgot_incomplete")} />
      ) : (
        state.error && <Banner status="error" title={state.error} />
      )}
      <form onSubmit={submit} noValidate>
        <VStack gap={4}>
          <TextInput
            label={t("common.email")}
            type="email"
            htmlName="email"
            autoComplete="username"
            placeholder={t("auth.email_placeholder")}
            value={email}
            onChange={setEmail}
            size="lg"
            hasAutoFocus
          />
          <Button
            label={t("auth.forgot_submit")}
            type="submit"
            variant="primary"
            size="lg"
            isLoading={isPending}
          />
        </VStack>
      </form>
      <Link href="/login" size="sm" color="secondary" type="supporting">
        {t("auth.back_to_sign_in")}
      </Link>
    </AuthCard>
  );
}
