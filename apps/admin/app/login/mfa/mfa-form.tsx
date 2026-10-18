"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type FormEvent, useActionState, useState, startTransition } from "react";

import { finishLogin } from "../actions";
import { AuthCard } from "../auth-card";

export function MfaForm() {
  const [code, setCode] = useState("");
  // The API tells the two apart by shape (6 digits or 16 characters); the switch only
  // changes what the field asks for.
  const [isRecovery, setIsRecovery] = useState(false);
  const [state, verify, isPending] = useActionState(finishLogin, {});

  // Read from the form, like the login: a password manager can fill the code without
  // the change events the state follows. Spaces never matter ("123 456").
  const submit = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const field = new FormData(e.currentTarget).get("code");
    const submitted = typeof field === "string" ? field.replace(/\s+/g, "") : "";
    setCode(submitted);
    if (submitted) startTransition(() => verify(submitted));
  };

  return (
    <AuthCard
      title="Two-factor authentication"
      subtitle={
        isRecovery
          ? "Enter one of the recovery codes you saved when you turned on two-factor authentication. Each works once."
          : "Enter the 6-digit code from your authenticator app."
      }
    >
      {state.error && <Banner status="error" title={state.error} />}
      <form onSubmit={submit} noValidate>
        <VStack gap={4}>
          <TextInput
            key={isRecovery ? "recovery" : "totp"}
            label={isRecovery ? "Recovery code" : "Code"}
            htmlName="code"
            autoComplete={isRecovery ? "off" : "one-time-code"}
            placeholder={isRecovery ? "XXXX-XXXX-XXXX-XXXX" : "123456"}
            value={code}
            onChange={setCode}
            size="lg"
            hasAutoFocus
          />
          <Button label="Verify" type="submit" variant="primary" size="lg" isLoading={isPending} />
        </VStack>
      </form>
      <HStack gap={4} hAlign="between" wrap="wrap">
        <Link
          href="#"
          size="sm"
          type="supporting"
          onClick={(e) => {
            e.preventDefault();
            setIsRecovery(!isRecovery);
            setCode("");
          }}
        >
          {isRecovery
            ? "Use your authenticator app"
            : "Lost your authenticator? Use a recovery code"}
        </Link>
        <Link href="/login" size="sm" color="secondary" type="supporting">
          Start over
        </Link>
      </HStack>
    </AuthCard>
  );
}
