"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useActionState, useState, startTransition } from "react";

import { login } from "./actions";
import { AuthCard } from "./auth-card";

export function LoginForm() {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  // The server action holds the credentials only long enough to call the API; the
  // session it gets back goes into an httpOnly cookie, never into this page.
  const [state, signIn, isPending] = useActionState(login, {});

  const submit = () => {
    if (!email || !password) return;
    startTransition(() => signIn({ email, password }));
  };

  return (
    <AuthCard title="Welcome back" subtitle="Sign in to your store's admin">
      {state.error && <Banner status="error" title={state.error} />}
      <VStack gap={2}>
        <TextInput
          label="Email"
          type="email"
          placeholder="name@company.com"
          value={email}
          onChange={setEmail}
          onEnter={submit}
          size="lg"
          hasAutoFocus
        />
        <TextInput
          label="Password"
          type="password"
          placeholder="Your password"
          value={password}
          onChange={setPassword}
          onEnter={submit}
          size="lg"
        />
      </VStack>
      <Button
        label="Sign in"
        variant="primary"
        size="lg"
        isLoading={isPending}
        isDisabled={!email || !password}
        onClick={submit}
      />
    </AuthCard>
  );
}
