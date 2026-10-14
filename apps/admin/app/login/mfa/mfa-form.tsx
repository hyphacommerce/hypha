"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Link } from "@astryxdesign/core/Link";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useActionState, useState, startTransition } from "react";

import { finishLogin } from "../actions";
import { AuthCard } from "../auth-card";

export function MfaForm() {
  const [code, setCode] = useState("");
  const [state, verify, isPending] = useActionState(finishLogin, {});

  const submit = () => {
    if (code.trim()) startTransition(() => verify(code));
  };

  return (
    <AuthCard
      title="Two-factor authentication"
      subtitle="Enter the 6-digit code from your authenticator app, or a recovery code."
    >
      {state.error && <Banner status="error" title={state.error} />}
      <TextInput
        label="Code"
        placeholder="123456"
        value={code}
        onChange={setCode}
        onEnter={submit}
        size="lg"
        hasAutoFocus
      />
      <Button
        label="Verify"
        variant="primary"
        size="lg"
        isLoading={isPending}
        isDisabled={!code.trim()}
        onClick={submit}
      />
      <Link href="/login" size="sm" color="secondary" type="supporting">
        Start over
      </Link>
    </AuthCard>
  );
}
