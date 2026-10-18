"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type FormEvent, startTransition, useActionState, useState } from "react";

import { NewPasswordFields, fieldText, newPasswordProblem } from "@/components/new-password";

import { acceptInvite } from "../login/actions";
import { AuthCard } from "../login/auth-card";

/** Accepting an invite: a name and a password, and the new staff member is signed in. */
export function InviteForm({ token }: { token: string }) {
  const [name, setName] = useState("");
  const [problem, setProblem] = useState<string>();
  const [state, accept, isPending] = useActionState(acceptInvite, {});

  const submit = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const fields = new FormData(e.currentTarget);
    const submittedName = fieldText(fields, "name").trim();
    const password = fieldText(fields, "new-password");
    const found = submittedName
      ? newPasswordProblem(password, fieldText(fields, "repeat-password"))
      : "Enter your name.";
    setProblem(found);
    if (found) return;
    startTransition(() => accept({ token, name: submittedName, password }));
  };

  return (
    <AuthCard
      title="Join the team"
      subtitle="You were invited to this store's admin. Choose the name your team sees and a password."
    >
      {(problem ?? state.error) && <Banner status="error" title={problem ?? state.error ?? ""} />}
      <form onSubmit={submit} noValidate>
        <VStack gap={4}>
          <TextInput
            label="Your name"
            htmlName="name"
            autoComplete="name"
            placeholder="Ana Horvat"
            value={name}
            onChange={setName}
            size="lg"
            hasAutoFocus
          />
          <NewPasswordFields label="Password" />
          <Button
            label="Join and sign in"
            type="submit"
            variant="primary"
            size="lg"
            isLoading={isPending}
          />
        </VStack>
      </form>
      <Link href="/login" size="sm" color="secondary" type="supporting">
        Already on the team? Sign in
      </Link>
    </AuthCard>
  );
}
