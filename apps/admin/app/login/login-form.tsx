"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type FormEvent, useActionState, useState, startTransition } from "react";

import { login } from "./actions";
import { AuthCard } from "./auth-card";

export function LoginForm({ notice }: { notice?: string }) {
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
    <AuthCard title="Welcome back" subtitle="Sign in to your store's admin">
      {isIncomplete ? (
        <Banner status="error" title="Enter your email and password." />
      ) : state.error ? (
        <Banner status="error" title={state.error} />
      ) : (
        notice && <Banner status="success" title={notice} />
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
            <HStack hAlign="end">
              <Link href="/forgot-password" size="sm" type="supporting">
                Forgot password?
              </Link>
            </HStack>
          </VStack>
          <Button label="Sign in" type="submit" variant="primary" size="lg" isLoading={isPending} />
        </VStack>
      </form>
    </AuthCard>
  );
}
