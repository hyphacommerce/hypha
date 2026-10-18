"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { type FormEvent, type ReactNode, startTransition, useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { NewPasswordFields, fieldText, newPasswordProblem } from "@/components/new-password";

import { logout } from "../../login/actions";
import { changePassword, revokeOtherSessions, revokeSession, updateName } from "./actions";

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

/** Who you are to the team; anyone may change their own name. */
export function ProfileCard({ name, email, role }: { name: string; email: string; role: string }) {
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
      description="Your name shows to your team. Your email is how you sign in; it can't change."
      action={
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

type Session = AdminSchemas["MySessionResponse"];

/** "Firefox on macOS" from a `User-Agent`, or `undefined` when it names neither. */
export function deviceName(userAgent: string | null | undefined): string | undefined {
  if (!userAgent) return undefined;
  // Order matters: Edge and Opera say Chrome too, Chrome says Safari.
  const browsers: [RegExp, string][] = [
    [/Edg\//, "Edge"],
    [/OPR\//, "Opera"],
    [/Firefox\//, "Firefox"],
    [/Chrome\//, "Chrome"],
    [/Safari\//, "Safari"],
    [/curl\//, "curl"],
  ];
  const systems: [RegExp, string][] = [
    [/iPhone|iPad/, "iOS"],
    [/Android/, "Android"],
    [/Mac OS X|Macintosh/, "macOS"],
    [/Windows/, "Windows"],
    [/CrOS/, "ChromeOS"],
    [/Linux/, "Linux"],
  ];
  const browser = browsers.find(([pattern]) => pattern.test(userAgent))?.[1];
  const system = systems.find(([pattern]) => pattern.test(userAgent))?.[1];
  if (browser && system) return `${browser} on ${system}`;
  return browser ?? system;
}

/**
 * Where the account is signed in: each session with its browser, address and last use;
 * signing one out, or every other one at once.
 */
export function SessionsCard({ sessions }: { sessions: Session[] }) {
  const [error, setError] = useState<string>();
  const [isConfirming, setIsConfirming] = useState(false);
  const [busy, setBusy] = useState<string>();
  const [isPending, startWork] = useTransition();
  const others = sessions.filter((s) => !s.current);

  const signOut = (session: Session) => {
    if (session.current) {
      startTransition(() => logout());
      return;
    }
    setBusy(session.id);
    startWork(async () => {
      setError((await revokeSession(session.id)).error);
      setBusy(undefined);
    });
  };
  const signOutOthers = () =>
    startWork(async () => {
      setError((await revokeOtherSessions()).error);
      setIsConfirming(false);
    });

  return (
    <AccountCard
      title="Sessions"
      description="Where your account is signed in. Sign out a device you don't recognise, then change your password."
      action={
        others.length > 0 && (
          <Button
            label="Sign out everywhere else"
            variant="secondary"
            size="sm"
            onClick={() => setIsConfirming(true)}
          />
        )
      }
    >
      {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
      <VStack gap={3}>
        {sessions.map((session, i) => (
          <VStack key={session.id} gap={3}>
            {i > 0 && <Divider />}
            <HStack gap={3} hAlign="between" vAlign="center">
              <VStack gap={1}>
                <HStack gap={2} vAlign="center">
                  <Text weight="medium">{deviceName(session.user_agent) ?? "Unknown device"}</Text>
                  {session.current && <Badge label="This device" variant="success" />}
                </HStack>
                <Text type="supporting" color="secondary">
                  {session.ip ? `${session.ip} · ` : ""}
                  {session.current ? (
                    "Active now"
                  ) : (
                    <>
                      Last active <DateTime value={session.last_used_at} short />
                    </>
                  )}
                  {" · Signed in "}
                  <DateTime value={session.created_at} short />
                </Text>
              </VStack>
              <Button
                label="Sign out"
                variant={session.current ? "secondary" : "ghost"}
                size="sm"
                isLoading={isPending && busy === session.id}
                onClick={() => signOut(session)}
              />
            </HStack>
          </VStack>
        ))}
      </VStack>
      <AlertDialog
        isOpen={isConfirming}
        onOpenChange={setIsConfirming}
        title="Sign out everywhere else?"
        description={`${others.length === 1 ? "The other device" : `The ${others.length} other devices`} must sign in again. This one stays signed in.`}
        actionLabel="Sign out others"
        isActionLoading={isPending}
        onAction={signOutOthers}
      />
    </AccountCard>
  );
}
