"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type FormEvent, startTransition, useActionState, useState } from "react";

import { fieldText } from "@/components/new-password";

import { requestPasswordReset } from "../login/actions";
import { AuthCard } from "../login/auth-card";

/** Asks for a reset link by email. */
export function ForgotForm() {
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
        title="Check your email"
        subtitle={`If ${state.sentTo} belongs to a staff account, a link to choose a new password is on its way. It works once, for a limited time.`}
      >
        <Link href="/login" size="sm" color="secondary" type="supporting">
          Back to sign in
        </Link>
      </AuthCard>
    );
  }

  return (
    <AuthCard
      title="Forgot your password?"
      subtitle="Enter your email and we'll send you a link to choose a new one."
    >
      {isIncomplete ? (
        <Banner status="error" title="Enter your email." />
      ) : (
        state.error && <Banner status="error" title={state.error} />
      )}
      <form onSubmit={submit} noValidate>
        <VStack gap={4}>
          <TextInput
            label="Email"
            type="email"
            htmlName="email"
            autoComplete="username"
            placeholder="name@company.com"
            value={email}
            onChange={setEmail}
            size="lg"
            hasAutoFocus
          />
          <Button
            label="Send the link"
            type="submit"
            variant="primary"
            size="lg"
            isLoading={isPending}
          />
        </VStack>
      </form>
      <Link href="/login" size="sm" color="secondary" type="supporting">
        Back to sign in
      </Link>
    </AuthCard>
  );
}
