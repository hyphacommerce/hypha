"use client";

// A new password typed twice, for the invite, the reset and the account page. The API
// holds the rule (8 to 128 characters); this only catches a typo before the round
// trip.

import { VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState } from "react";

export const PASSWORD_MIN = 8;

/** What's wrong with the pair, for the form to show, or nothing. */
export function newPasswordProblem(password: string, repeated: string): string | undefined {
  if (password.length < PASSWORD_MIN) return `Use at least ${PASSWORD_MIN} characters.`;
  if (password !== repeated) return "The two passwords don't match.";
  return undefined;
}

/** A form field's text: text inputs give strings, anything else counts as empty. */
export function fieldText(fields: FormData, name: string): string {
  const value = fields.get(name);
  return typeof value === "string" ? value : "";
}

export function NewPasswordFields({
  label = "New password",
  size = "lg",
}: {
  label?: string;
  size?: "md" | "lg";
}) {
  const [password, setPassword] = useState("");
  const [repeated, setRepeated] = useState("");
  return (
    <VStack gap={2}>
      <TextInput
        label={label}
        type="password"
        htmlName="new-password"
        autoComplete="new-password"
        description={`At least ${PASSWORD_MIN} characters.`}
        value={password}
        onChange={setPassword}
        size={size}
      />
      <TextInput
        label="Repeat it"
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
