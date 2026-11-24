"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { type SubmitEvent, startTransition, useActionState, useState } from "react";

import { NewPasswordFields, fieldText, newPasswordProblem } from "@/components/new-password";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { acceptInvite } from "../login/actions";
import { AuthCard } from "../login/auth-card";

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
  const t = useT<"auth">();
  // The day in UTC: the reader isn't in the store's time zone yet, and the server and
  // the browser must render the same text.
  const format = useFormat();
  const [name, setName] = useState("");
  const [problem, setProblem] = useState<string>();
  const [state, accept, isPending] = useActionState(acceptInvite, {});

  const submit = (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    const fields = new FormData(e.currentTarget);
    const submittedName = fieldText(fields, "name").trim();
    const password = fieldText(fields, "new-password");
    const found = submittedName
      ? newPasswordProblem(password, fieldText(fields, "repeat-password"), t)
      : t("auth.invite_name_required");
    setProblem(found);
    if (found) return;
    startTransition(() => accept({ token, name: submittedName, password }));
  };

  return (
    <AuthCard
      title={t("auth.invite_title", { store: invite.store_name })}
      subtitle={
        invite.invited_by
          ? t("auth.invite_subtitle_invited_by", { name: invite.invited_by })
          : t("auth.invite_subtitle")
      }
    >
      <MetadataList>
        <MetadataListItem label={t("common.email")}>{invite.email}</MetadataListItem>
        <MetadataListItem label={t("common.role")}>{invite.role}</MetadataListItem>
        <MetadataListItem label={t("auth.invite_expires")}>
          {format.date(invite.expires_at, "long")}
        </MetadataListItem>
      </MetadataList>
      {(problem ?? state.error) && <Banner status="error" title={problem ?? state.error ?? ""} />}
      <form onSubmit={submit} noValidate>
        <VStack gap={4}>
          <TextInput
            label={t("auth.invite_name")}
            htmlName="name"
            autoComplete="name"
            placeholder={t("auth.invite_name_placeholder")}
            value={name}
            onChange={setName}
            size="lg"
            hasAutoFocus
          />
          <NewPasswordFields label={t("common.password")} />
          <Button
            label={t("auth.invite_submit")}
            type="submit"
            variant="primary"
            size="lg"
            isLoading={isPending}
          />
        </VStack>
      </form>
      <Link href="/login" size="sm" color="secondary" type="supporting">
        {t("auth.invite_sign_in_instead")}
      </Link>
    </AuthCard>
  );
}
