"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type SubmitEvent, useActionState, useState, startTransition } from "react";

import { useT } from "@/lib/use-t";

import { login } from "./actions";
import { AuthCard } from "./auth-card";

export function LoginForm({ notice }: { notice?: string }) {
  const t = useT<"auth">();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [isIncomplete, setIsIncomplete] = useState(false);
  // The server action holds the credentials only long enough to call the API; the
  // session it gets back goes into an httpOnly cookie, never into this page.
  const [state, signIn, isPending] = useActionState(login, {});

  // A real form, read when it's submitted: browsers and password managers fill saved
  // credentials into the fields without the change events React's state follows, so
  // the state alone would send nothing (or what was there before) while the fields
  // show the saved login.
  const submit = (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    const fields = new FormData(e.currentTarget);
    const emailField = fields.get("email");
    const passwordField = fields.get("password");
    // Text inputs give strings; anything else (a file, nothing) counts as empty.
    const submittedEmail = typeof emailField === "string" ? emailField.trim() : "";
    const submittedPassword = typeof passwordField === "string" ? passwordField : "";
    setEmail(submittedEmail);
    setPassword(submittedPassword);
    if (!submittedEmail || !submittedPassword) {
      setIsIncomplete(true);
      return;
    }
    setIsIncomplete(false);
    startTransition(() => signIn({ email: submittedEmail, password: submittedPassword }));
  };

  return (
    <AuthCard title={t("auth.login_title")} subtitle={t("auth.login_subtitle")}>
      {isIncomplete ? (
        <Banner status="error" title={t("auth.login_incomplete")} />
      ) : state.error ? (
        <Banner status="error" title={state.error} />
      ) : (
        notice && <Banner status="success" title={notice} />
      )}
      <form onSubmit={submit} noValidate>
        <VStack gap={4}>
          <VStack gap={2}>
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
            <TextInput
              label={t("common.password")}
              type="password"
              htmlName="password"
              autoComplete="current-password"
              placeholder={t("auth.login_password_placeholder")}
              value={password}
              onChange={setPassword}
              size="lg"
            />
            <HStack hAlign="end">
              <Link href="/forgot-password" size="sm" type="supporting">
                {t("auth.login_forgot_password")}
              </Link>
            </HStack>
          </VStack>
          <Button
            label={t("auth.sign_in")}
            type="submit"
            variant="primary"
            size="lg"
            isLoading={isPending}
          />
        </VStack>
      </form>
    </AuthCard>
  );
}
