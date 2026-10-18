"use client";

// Two-factor authentication on the account page (design note 0008, sections 3 and 6):
// set up an authenticator (password → scan → first code → recovery codes), get new
// recovery codes, or turn it off. Each code-taking step accepts a recovery code too.

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import { QrCode } from "@/components/qr-code";

import { AccountCard } from "./account-cards";
import {
  type Result,
  confirmTotp,
  disableTotp,
  regenerateRecoveryCodes,
  startTotp,
} from "./actions";

type Step =
  | { kind: "password" }
  | { kind: "scan"; secret: string; uri: string }
  | { kind: "codes"; codes: string[]; isNew: boolean }
  | { kind: "regenerate" }
  | { kind: "disable" };

/** "JBSWY3DPEHPK3PXP" → "JBSW Y3DP EHPK 3PXP", easier to type into an app. */
function grouped(secret: string): string {
  return secret.replace(/(.{4})(?=.)/g, "$1 ");
}

export function TwoFactorCard({ isEnabled }: { isEnabled: boolean }) {
  const [step, setStep] = useState<Step>();
  const close = () => setStep(undefined);

  return (
    <AccountCard
      id="two-factor"
      title="Two-factor authentication"
      description={
        isEnabled
          ? "Signing in asks for a code from your authenticator app, or one of your recovery codes."
          : "Protect your account with a code from an authenticator app (1Password, Google Authenticator, Authy, …) as well as your password."
      }
      action={
        <Badge label={isEnabled ? "On" : "Off"} variant={isEnabled ? "success" : "warning"} />
      }
    >
      <HStack gap={2} wrap="wrap">
        {isEnabled ? (
          <>
            <Button
              label="New recovery codes"
              variant="secondary"
              onClick={() => setStep({ kind: "regenerate" })}
            />
            <Button
              label="Turn off"
              variant="destructive"
              onClick={() => setStep({ kind: "disable" })}
            />
          </>
        ) : (
          <Button
            label="Set up two-factor"
            variant="primary"
            onClick={() => setStep({ kind: "password" })}
          />
        )}
      </HStack>
      {/* Outside the branches above: turning 2FA on re-renders the card as "On" while the
          dialog still shows the recovery codes. */}
      {step && <TwoFactorDialog step={step} onStep={setStep} onClose={close} />}
    </AccountCard>
  );
}

function TwoFactorDialog({
  step,
  onStep,
  onClose,
}: {
  step: Step;
  onStep: (step: Step) => void;
  onClose: () => void;
}) {
  const title = {
    password: "Set up two-factor authentication",
    scan: "Scan the code",
    codes: "Save your recovery codes",
    regenerate: "New recovery codes",
    disable: "Turn off two-factor authentication",
  }[step.kind];
  const onOpenChange = (isOpen: boolean) => !isOpen && onClose();

  return (
    <Dialog isOpen onOpenChange={onOpenChange} width={520} purpose="form">
      <DialogHeader title={title} onOpenChange={onOpenChange} />
      <VStack gap={4} padding={4}>
        {step.kind === "password" && (
          <AskOnce
            key="password"
            label="Your password"
            description="To check it's you before an authenticator is added."
            type="password"
            autoComplete="current-password"
            submitLabel="Continue"
            onSubmit={async (password) => {
              const result = await startTotp(password);
              if (result.secret && result.uri) {
                onStep({ kind: "scan", secret: result.secret, uri: result.uri });
              }
              return result;
            }}
            onCancel={onClose}
          />
        )}
        {step.kind === "scan" && (
          <VStack gap={4}>
            <Text>
              Scan this with your authenticator app, then enter the 6-digit code it shows.
            </Text>
            <HStack hAlign="center">
              <QrCode value={step.uri} label="QR code for your authenticator app" />
            </HStack>
            <VStack gap={1}>
              <Text type="supporting" color="secondary">
                Can't scan it? Enter this key in the app instead (time-based, 6 digits):
              </Text>
              <Text type="code">{grouped(step.secret)}</Text>
            </VStack>
            <AskOnce
              key="scan"
              label="Code from the app"
              autoComplete="one-time-code"
              placeholder="123456"
              submitLabel="Turn on"
              onSubmit={async (code) => {
                const result = await confirmTotp(code);
                if (result.codes) onStep({ kind: "codes", codes: result.codes, isNew: true });
                return result;
              }}
              onCancel={onClose}
            />
          </VStack>
        )}
        {step.kind === "regenerate" && (
          <AskOnce
            key="regenerate"
            label="Code from your authenticator, or a recovery code"
            description="Your current recovery codes stop working once the new ones are made."
            autoComplete="one-time-code"
            submitLabel="Make new codes"
            onSubmit={async (code) => {
              const result = await regenerateRecoveryCodes(code);
              if (result.codes) onStep({ kind: "codes", codes: result.codes, isNew: false });
              return result;
            }}
            onCancel={onClose}
          />
        )}
        {step.kind === "disable" && (
          <AskOnce
            key="disable"
            label="Code from your authenticator, or a recovery code"
            description="Signing in will then ask only for your password, and your recovery codes stop working."
            autoComplete="one-time-code"
            submitLabel="Turn off"
            isDestructive
            onSubmit={async (code) => {
              const result = await disableTotp(code);
              if (!result.error) onClose();
              return result;
            }}
            onCancel={onClose}
          />
        )}
        {step.kind === "codes" && (
          <RecoveryCodesView codes={step.codes} isNew={step.isNew} onDone={onClose} />
        )}
      </VStack>
    </Dialog>
  );
}

/** One field and a button, with the API's answer shown above it. */
function AskOnce({
  label,
  description,
  type,
  autoComplete,
  placeholder,
  submitLabel,
  isDestructive,
  onSubmit,
  onCancel,
}: {
  label: string;
  description?: string;
  type?: "password";
  autoComplete: string;
  placeholder?: string;
  submitLabel: string;
  isDestructive?: boolean;
  onSubmit: (value: string) => Promise<Result>;
  onCancel: () => void;
}) {
  const [value, setValue] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  // Spaces never matter in a password-less code ("123 456", "ABCD EFGH …").
  const submitted = type === "password" ? value : value.replace(/\s+/g, "");

  const submit = () => {
    if (!submitted) return;
    startTransition(async () => setError((await onSubmit(submitted)).error));
  };

  return (
    <VStack gap={3}>
      {error && <Banner status="error" title={error} />}
      <TextInput
        label={label}
        description={description}
        type={type}
        autoComplete={autoComplete}
        placeholder={placeholder}
        value={value}
        onChange={setValue}
        onEnter={submit}
        hasAutoFocus
      />
      <HStack gap={2} hAlign="end">
        <Button label="Cancel" variant="ghost" onClick={onCancel} />
        <Button
          label={submitLabel}
          variant={isDestructive ? "destructive" : "primary"}
          isLoading={isPending}
          isDisabled={!submitted}
          onClick={submit}
        />
      </HStack>
    </VStack>
  );
}

/** The recovery codes, shown this once, with ways to keep them. */
function RecoveryCodesView({
  codes,
  isNew,
  onDone,
}: {
  codes: string[];
  isNew: boolean;
  onDone: () => void;
}) {
  const [isCopied, setIsCopied] = useState(false);
  const text = `Hypha admin recovery codes\nEach works once, if you lose your authenticator.\n\n${codes.join("\n")}\n`;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(codes.join("\n"));
      setIsCopied(true);
    } catch {
      // Clipboard refused (permissions, an insecure origin): the codes stay on screen.
    }
  };
  const download = () => {
    const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
    const link = document.createElement("a");
    link.href = url;
    link.download = "hypha-recovery-codes.txt";
    link.click();
    URL.revokeObjectURL(url);
  };

  return (
    <VStack gap={4}>
      <Banner
        status={isNew ? "success" : "info"}
        title={
          isNew ? "Two-factor authentication is on." : "Your old recovery codes no longer work."
        }
        description="Keep these somewhere safe, like a password manager. Each works once to sign in without your authenticator. They won't be shown again."
      />
      <Grid columns={{ minWidth: 180, repeat: "fill" }} gap={2}>
        {codes.map((code) => (
          <Text key={code} type="code">
            {code}
          </Text>
        ))}
      </Grid>
      <HStack gap={2} hAlign="between" wrap="wrap">
        <HStack gap={2}>
          <Button label={isCopied ? "Copied" : "Copy"} variant="secondary" onClick={copy} />
          <Button label="Download" variant="secondary" onClick={download} />
        </HStack>
        <Button label="I've saved them" variant="primary" onClick={onDone} />
      </HStack>
    </VStack>
  );
}
