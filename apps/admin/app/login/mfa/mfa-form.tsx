"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type FormEvent, useActionState, useState, startTransition } from "react";

import { finishLogin } from "../actions";
import { AuthCard } from "../auth-card";

export function MfaForm() {
  const [code, setCode] = useState("");
  const [state, verify, isPending] = useActionState(finishLogin, {});

  // Read from the form, like the login: a password manager can fill the code without
  // the change events the state follows.
  const submit = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const field = new FormData(e.currentTarget).get("code");
    const submitted = typeof field === "string" ? field.trim() : "";
    setCode(submitted);
    if (submitted) startTransition(() => verify(submitted));
  };

  return (
    <AuthCard
      title="Two-factor authentication"
      subtitle="Enter the 6-digit code from your authenticator app, or a recovery code."
    >
      {state.error && <Banner status="error" title={state.error} />}
      <form onSubmit={submit} noValidate>
        <VStack gap={4}>
          <TextInput
            label="Code"
            htmlName="code"
            autoComplete="one-time-code"
            placeholder="123456"
            value={code}
            onChange={setCode}
            size="lg"
            hasAutoFocus
          />
          <Button label="Verify" type="submit" variant="primary" size="lg" isLoading={isPending} />
        </VStack>
      </form>
      <Link href="/login" size="sm" color="secondary" type="supporting">
        Start over
      </Link>
    </AuthCard>
  );
}
