"use client";

// Signing in with an emailed code: the email, then the six digits. The email's link
// brings both along (`/account/login?email=…&code=…`), so the second step is already
// filled in. Centred in a card, like a sign-in page should be.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Heading } from "@astryxdesign/core/Heading";
import { Icon } from "@astryxdesign/core/Icon";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { EnvelopeIcon, KeyIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { requestCode, signIn } from "@/app/[country]/(shop)/account/actions";
import { useT } from "@/lib/use-t";
import { useLocale } from "@/components/locale";

export function SignInForm({
  storeName,
  initialEmail = "",
  initialCode = "",
}: {
  storeName: string;
  initialEmail?: string;
  initialCode?: string;
}) {
  const locale = useLocale();
  const t = useT<"account">();
  const [email, setEmail] = useState(initialEmail);
  const [code, setCode] = useState(initialCode);
  const [step, setStep] = useState<"email" | "code">(initialEmail ? "code" : "email");
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();

  const ask = () => {
    if (email.trim() === "") return;
    start(async () => {
      const result = await requestCode(locale, email);
      setError(result?.error);
      if (!result) setStep("code");
    });
  };
  const enter = () => {
    if (code.trim() === "") return;
    start(async () => {
      // Signed in, the action redirects: only a refusal comes back.
      const result = await signIn(locale, email, code);
      setError(result?.error);
    });
  };

  return (
    <div className="shop-sign-in">
      <Card padding={6}>
        <VStack gap={5} hAlign="center">
          <div className="shop-sign-in-icon">
            <Icon icon={step === "email" ? EnvelopeIcon : KeyIcon} size="lg" />
          </div>
          <div className="shop-sign-in-copy">
            <VStack gap={1} hAlign="center">
              <Heading level={1} type="display-3">
                {step === "email"
                  ? t("account.sign_in_to", { store: storeName })
                  : t("account.check_email")}
              </Heading>
              <Text color="secondary">
                {step === "email"
                  ? t("account.enter_email")
                  : t("account.code_sent", { email: email.trim() })}
              </Text>
            </VStack>
          </div>
          {step === "email" ? (
            <form
              className="shop-sign-in-form"
              onSubmit={(e) => {
                e.preventDefault();
                ask();
              }}
            >
              <VStack gap={4}>
                <TextInput
                  size="lg"
                  label={t("account.email")}
                  type="email"
                  isRequired
                  hasAutoFocus
                  value={email}
                  onChange={setEmail}
                />
                {error && <Banner status="error" title={error} />}
                <Button
                  label={t("account.send_code")}
                  variant="primary"
                  size="lg"
                  type="submit"
                  width="100%"
                  isLoading={pending}
                />
              </VStack>
            </form>
          ) : (
            <form
              className="shop-sign-in-form"
              onSubmit={(e) => {
                e.preventDefault();
                enter();
              }}
            >
              <VStack gap={4}>
                <TextInput
                  size="lg"
                  label={t("account.code")}
                  autoComplete="one-time-code"
                  isRequired
                  hasAutoFocus
                  value={code}
                  onChange={setCode}
                />
                {error && <Banner status="error" title={error} />}
                <Button
                  label={t("account.sign_in")}
                  variant="primary"
                  size="lg"
                  type="submit"
                  width="100%"
                  isLoading={pending}
                />
                <HStack gap={2} hAlign="center" wrap="wrap">
                  <Button
                    label={t("account.use_another_email")}
                    variant="ghost"
                    size="sm"
                    clickAction={() => {
                      setStep("email");
                      setCode("");
                      setError(undefined);
                    }}
                  />
                  <Button
                    label={t("account.send_new_code")}
                    variant="ghost"
                    size="sm"
                    isLoading={pending}
                    clickAction={ask}
                  />
                </HStack>
              </VStack>
            </form>
          )}
          <div className="shop-sign-in-note">
            <Text type="supporting" color="secondary">
              {t("account.new_here")}
            </Text>
          </div>
        </VStack>
      </Card>
    </div>
  );
}
