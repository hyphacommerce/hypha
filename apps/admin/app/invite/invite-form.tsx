"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { type FormEvent, startTransition, useActionState, useState } from "react";

import { NewPasswordFields, fieldText, newPasswordProblem } from "@/components/new-password";

import { acceptInvite } from "../login/actions";
import { AuthCard } from "../login/auth-card";

/**
 * "20 October 2026" (UTC): the reader isn't in the store's time zone yet, and the server
 * and the browser must render the same text.
 */
const DAY = new Intl.DateTimeFormat("en-GB", { dateStyle: "long", timeZone: "UTC" });

/**
 * Accepting an invite: what it is (the store, the email, the role, who sent it), then a
 * name and a password, and the new staff member is signed in.
 */
export function InviteForm({
  token,
  invite,
}: {
  token: string;
  invite: AdminSchemas["InviteLookupResponse"];
}) {
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
      title={`Join ${invite.store_name}`}
      subtitle={`${invite.invited_by ? `${invite.invited_by} invited you` : "You were invited"} to this store's admin. Choose the name your team sees and a password.`}
    >
      <MetadataList>
        <MetadataListItem label="Email">{invite.email}</MetadataListItem>
        <MetadataListItem label="Role">{invite.role}</MetadataListItem>
        <MetadataListItem label="Link works until">
          {DAY.format(new Date(invite.expires_at))}
        </MetadataListItem>
      </MetadataList>
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
