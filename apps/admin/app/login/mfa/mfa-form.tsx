"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type FormEvent, useActionState, useState, startTransition } from "react";

import { useT } from "@/lib/use-t";

import { finishLogin } from "../actions";
import { AuthCard } from "../auth-card";

// What a code looks like: the same in every language.
const CODE_EXAMPLE = "123456";
const RECOVERY_CODE_EXAMPLE = "XXXX-XXXX-XXXX-XXXX";

export function MfaForm() {
  const t = useT<"auth">();
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
      title={t("auth.mfa_title")}
      subtitle={isRecovery ? t("auth.mfa_subtitle_recovery") : t("auth.mfa_subtitle_code")}
    >
      {state.error && <Banner status="error" title={state.error} />}
      <form onSubmit={submit} noValidate>
        <VStack gap={4}>
          <TextInput
            key={isRecovery ? "recovery" : "totp"}
            label={isRecovery ? t("auth.mfa_recovery_code") : t("common.code")}
            htmlName="code"
            autoComplete={isRecovery ? "off" : "one-time-code"}
            placeholder={isRecovery ? RECOVERY_CODE_EXAMPLE : CODE_EXAMPLE}
            value={code}
            onChange={setCode}
            size="lg"
            hasAutoFocus
          />
          <Button
            label={t("auth.mfa_verify")}
            type="submit"
            variant="primary"
            size="lg"
            isLoading={isPending}
          />
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
          {isRecovery ? t("auth.mfa_use_app") : t("auth.mfa_use_recovery")}
        </Link>
        <Link href="/login" size="sm" color="secondary" type="supporting">
          {t("auth.mfa_start_over")}
        </Link>
      </HStack>
    </AuthCard>
  );
}
