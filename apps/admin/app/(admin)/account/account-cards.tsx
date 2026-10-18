"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type FormEvent, type ReactNode, startTransition, useState, useTransition } from "react";

import { NewPasswordFields, fieldText, newPasswordProblem } from "@/components/new-password";

import { logout } from "../../login/actions";
import { changePassword, updateName } from "./actions";

export function AccountCard({
  title,
  description,
  id,
  action,
  children,
}: {
  title: string;
  description?: ReactNode;
  id?: string;
  action?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <Card>
      <VStack gap={4}>
        <HStack gap={3} hAlign="between" vAlign="start" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2" id={id}>
              {title}
            </Text>
            {description && (
              <Text type="supporting" color="secondary">
                {description}
              </Text>
            )}
          </VStack>
          {action}
        </HStack>
        {children}
      </VStack>
    </Card>
  );
}

/** Who you are to the team; the name is editable with `users:write`. */
export function ProfileCard({
  name,
  email,
  role,
  canRename,
}: {
  name: string;
  email: string;
  role: string;
  canRename: boolean;
}) {
  const [draft, setDraft] = useState<string>();
  const [error, setError] = useState<string>();
  const [isPending, startSaving] = useTransition();

  const save = () => {
    const next = draft?.trim() ?? "";
    if (!next) {
      setError("Enter a name.");
      return;
    }
    startSaving(async () => {
      const result = await updateName(next);
      setError(result.error);
      if (!result.error) setDraft(undefined);
    });
  };

  return (
    <AccountCard
      title="Profile"
      description={
        canRename
          ? "Your name shows to your team. Your email is how you sign in; it can't change."
          : "Your name shows to your team. Ask someone who manages staff to change it; your email can't change."
      }
      action={
        canRename &&
        draft === undefined && (
          <Button label="Edit name" variant="secondary" size="sm" onClick={() => setDraft(name)} />
        )
      }
    >
      {error && <Banner status="error" title={error} />}
      {draft === undefined ? (
        <MetadataList>
          <MetadataListItem label="Name">{name}</MetadataListItem>
          <MetadataListItem label="Email">{email}</MetadataListItem>
          <MetadataListItem label="Role">{role}</MetadataListItem>
        </MetadataList>
      ) : (
        <VStack gap={3}>
          <TextInput label="Name" value={draft} onChange={setDraft} onEnter={save} hasAutoFocus />
          <HStack gap={2} hAlign="end">
            <Button
              label="Cancel"
              variant="ghost"
              onClick={() => {
                setDraft(undefined);
                setError(undefined);
              }}
            />
            <Button label="Save" variant="primary" isLoading={isPending} onClick={save} />
          </HStack>
        </VStack>
      )}
    </AccountCard>
  );
}

/** A new password, with the current one; other devices are signed out. */
export function PasswordCard() {
  const [current, setCurrent] = useState("");
  const [message, setMessage] = useState<{ text: string; isError: boolean }>();
  const [isPending, startSaving] = useTransition();
  // Bumped after a change, so the fields start empty again.
  const [round, setRound] = useState(0);

  const submit = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const fields = new FormData(e.currentTarget);
    const currentPassword = fieldText(fields, "current-password");
    const password = fieldText(fields, "new-password");
    const problem = currentPassword
      ? newPasswordProblem(password, fieldText(fields, "repeat-password"))
      : "Enter your current password.";
    if (problem) {
      setMessage({ text: problem, isError: true });
      return;
    }
    startSaving(async () => {
      const result = await changePassword({
        current_password: currentPassword,
        new_password: password,
      });
      if (result.error) {
        setMessage({ text: result.error, isError: true });
        return;
      }
      setMessage({
        text: "Password changed. Every other device signed in to your account was signed out.",
        isError: false,
      });
      setCurrent("");
      setRound((n) => n + 1);
    });
  };

  return (
    <AccountCard
      title="Password"
      description="Changing it signs you out everywhere else, and ends any reset link sent before."
    >
      {message && (
        <Banner
          status={message.isError ? "error" : "success"}
          title={message.text}
          onDismiss={() => setMessage(undefined)}
        />
      )}
      <form onSubmit={submit} noValidate key={round}>
        <VStack gap={3}>
          <TextInput
            label="Current password"
            type="password"
            htmlName="current-password"
            autoComplete="current-password"
            value={current}
            onChange={setCurrent}
          />
          <NewPasswordFields size="md" />
          <HStack hAlign="end">
            <Button label="Change password" type="submit" variant="primary" isLoading={isPending} />
          </HStack>
        </VStack>
      </form>
    </AccountCard>
  );
}

/** Signing out here, and how to sign out everywhere else. */
export function SessionsCard() {
  return (
    <AccountCard
      title="Sessions"
      description="A sign-in lasts until you sign out or it expires. To sign out every other device at once, change your password."
      action={
        <Button
          label="Sign out"
          variant="secondary"
          onClick={() => startTransition(() => logout())}
        />
      }
    />
  );
}
