"use client";

// Signing in with an emailed code (design note 0034, §2): the email, then the six digits.
// The email's link brings both along (`/account/login?email=…&code=…`), so the second
// step is already filled in.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import { requestCode, signIn } from "@/app/[country]/account/actions";

export function SignInForm({
  country,
  initialEmail = "",
  initialCode = "",
}: {
  country: string;
  initialEmail?: string;
  initialCode?: string;
}) {
  const [email, setEmail] = useState(initialEmail);
  const [code, setCode] = useState(initialCode);
  const [step, setStep] = useState<"email" | "code">(initialEmail ? "code" : "email");
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();

  const ask = () => {
    if (email.trim() === "") return;
    start(async () => {
      const result = await requestCode(country, email);
      setError(result?.error);
      if (!result) setStep("code");
    });
  };
  const enter = () => {
    if (code.trim() === "") return;
    start(async () => {
      // Signed in, the action redirects: only a refusal comes back.
      const result = await signIn(country, email, code);
      setError(result?.error);
    });
  };

  return (
    <VStack gap={4} maxWidth={420}>
      {step === "email" ? (
        <>
          <Text color="secondary">
            Enter your email and we&apos;ll send you a six-digit code to sign in with. No password
            needed.
          </Text>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              ask();
            }}
          >
            <VStack gap={4}>
              <TextInput
                size="lg"
                label="Email"
                type="email"
                isRequired
                value={email}
                onChange={setEmail}
              />
              {error && <Banner status="error" title={error} />}
              <div>
                <Button
                  label="Send me a code"
                  variant="primary"
                  size="lg"
                  type="submit"
                  isLoading={pending}
                />
              </div>
            </VStack>
          </form>
        </>
      ) : (
        <>
          <Text color="secondary">
            We sent a code to {email.trim()}. Enter it here; it works for ten minutes.
          </Text>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              enter();
            }}
          >
            <VStack gap={4}>
              <TextInput
                size="lg"
                label="Code"
                autoComplete="one-time-code"
                isRequired
                value={code}
                onChange={setCode}
              />
              {error && <Banner status="error" title={error} />}
              <div>
                <Button
                  label="Sign in"
                  variant="primary"
                  size="lg"
                  type="submit"
                  isLoading={pending}
                />
              </div>
            </VStack>
          </form>
          <HStack gap={2} wrap="wrap">
            <Button
              label="Use another email"
              variant="ghost"
              clickAction={() => {
                setStep("email");
                setCode("");
                setError(undefined);
              }}
            />
            <Button label="Send a new code" variant="ghost" isLoading={pending} clickAction={ask} />
          </HStack>
        </>
      )}
    </VStack>
  );
}
