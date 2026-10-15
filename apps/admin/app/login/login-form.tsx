"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type FormEvent, useActionState, useState, startTransition } from "react";

import { login } from "./actions";
import { AuthCard } from "./auth-card";

export function LoginForm() {
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
  const submit = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const fields = new FormData(e.currentTarget);
    const submittedEmail = String(fields.get("email") ?? "").trim();
    const submittedPassword = String(fields.get("password") ?? "");
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
    <AuthCard title="Welcome back" subtitle="Sign in to your store's admin">
      {isIncomplete ? (
        <Banner status="error" title="Enter your email and password." />
      ) : (
        state.error && <Banner status="error" title={state.error} />
      )}
      <form onSubmit={submit} noValidate>
        <VStack gap={4}>
          <VStack gap={2}>
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
            <TextInput
              label="Password"
              type="password"
              htmlName="password"
              autoComplete="current-password"
              placeholder="Your password"
              value={password}
              onChange={setPassword}
              size="lg"
            />
          </VStack>
          <Button label="Sign in" type="submit" variant="primary" size="lg" isLoading={isPending} />
        </VStack>
      </form>
    </AuthCard>
  );
}
