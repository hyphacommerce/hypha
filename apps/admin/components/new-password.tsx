"use client";

// A new password typed twice, for the invite, the reset and the account page. The API
// holds the rule (8 to 128 characters); this only catches a typo before the round
// trip.

import { VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState } from "react";

import type { TFor } from "@/lib/messages";
import { useT } from "@/lib/use-t";

export const PASSWORD_MIN = 8;

/**
 * What's wrong with the pair, for the form to show, or nothing. In the language of `t`
 * (the form's `useT()`); English without one.
 */
export function newPasswordProblem(
  password: string,
  repeated: string,
  t: TFor<"components">,
): string | undefined {
  if (password.length < PASSWORD_MIN) {
    return t("components.new_password_too_short", { min: PASSWORD_MIN });
  }
  if (password !== repeated) return t("components.new_password_mismatch");
  return undefined;
}

/** A form field's text: text inputs give strings, anything else counts as empty. */
export function fieldText(fields: FormData, name: string): string {
  const value = fields.get(name);
  return typeof value === "string" ? value : "";
}

export function NewPasswordFields({ label, size = "lg" }: { label?: string; size?: "md" | "lg" }) {
  const t = useT();
  const [password, setPassword] = useState("");
  const [repeated, setRepeated] = useState("");
  return (
    <VStack gap={2}>
      <TextInput
        label={label ?? t("components.new_password_label")}
        type="password"
        htmlName="new-password"
        autoComplete="new-password"
        description={t("components.new_password_hint", { min: PASSWORD_MIN })}
        value={password}
        onChange={setPassword}
        size={size}
      />
      <TextInput
        label={t("components.new_password_repeat")}
        type="password"
        htmlName="repeat-password"
        autoComplete="new-password"
        value={repeated}
        onChange={setRepeated}
        size={size}
      />
    </VStack>
  );
}
