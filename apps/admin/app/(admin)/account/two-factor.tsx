"use client";

// Two-factor authentication on the account page: set up an authenticator (password →
// scan → first code → recovery codes), get new recovery codes, or turn it off. Each
// code-taking step accepts a recovery code too.

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
import { useT } from "@/lib/use-t";

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

/** What a code from an authenticator looks like: the same in every language. */
const CODE_EXAMPLE = "123456";

export function TwoFactorCard({ isEnabled }: { isEnabled: boolean }) {
  const t = useT<"account">();
  const [step, setStep] = useState<Step>();
  const close = () => setStep(undefined);

  return (
    <AccountCard
      id="two-factor"
      title={t("account.two_factor_title")}
      description={
        isEnabled ? t("account.two_factor_description_on") : t("account.two_factor_description_off")
      }
      action={
        <Badge
          label={isEnabled ? t("common.on") : t("common.off")}
          variant={isEnabled ? "success" : "warning"}
        />
      }
    >
      <HStack gap={2} wrap="wrap">
        {isEnabled ? (
          <>
            <Button
              label={t("account.two_factor_new_codes")}
              variant="secondary"
              onClick={() => setStep({ kind: "regenerate" })}
            />
            <Button
              label={t("common.turn_off")}
              variant="destructive"
              onClick={() => setStep({ kind: "disable" })}
            />
          </>
        ) : (
          <Button
            label={t("account.two_factor_set_up")}
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
  const t = useT<"account">();
  const title = t(
    (
      {
        password: "account.two_factor_setup_title",
        scan: "account.two_factor_scan_title",
        codes: "account.two_factor_codes_title",
        regenerate: "account.two_factor_new_codes",
        disable: "account.two_factor_disable_title",
      } as const
    )[step.kind],
  );
  const onOpenChange = (isOpen: boolean) => !isOpen && onClose();

  return (
    <Dialog isOpen onOpenChange={onOpenChange} width={520} purpose="form">
      <DialogHeader title={title} onOpenChange={onOpenChange} />
      <VStack gap={4} padding={4}>
        {step.kind === "password" && (
          <AskOnce
            key="password"
            label={t("account.two_factor_password")}
            description={t("account.two_factor_password_description")}
            type="password"
            autoComplete="current-password"
            submitLabel={t("common.continue")}
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
            <Text>{t("account.two_factor_scan_instructions")}</Text>
            <HStack hAlign="center">
              <QrCode value={step.uri} label={t("account.two_factor_qr_label")} />
            </HStack>
            <VStack gap={1}>
              <Text type="supporting" color="secondary">
                {t("account.two_factor_scan_manual")}
              </Text>
              <Text type="code">{grouped(step.secret)}</Text>
            </VStack>
            <AskOnce
              key="scan"
              label={t("account.two_factor_app_code")}
              autoComplete="one-time-code"
              placeholder={CODE_EXAMPLE}
              submitLabel={t("common.turn_on")}
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
            label={t("account.two_factor_code_or_recovery")}
            description={t("account.two_factor_regenerate_description")}
            autoComplete="one-time-code"
            submitLabel={t("account.two_factor_regenerate_submit")}
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
            label={t("account.two_factor_code_or_recovery")}
            description={t("account.two_factor_disable_description")}
            autoComplete="one-time-code"
            submitLabel={t("common.turn_off")}
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
  const t = useT<"account">();
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
        <Button label={t("common.cancel")} variant="ghost" onClick={onCancel} />
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
  const t = useT<"account">();
  const [isCopied, setIsCopied] = useState(false);
  const text = `${t("account.two_factor_codes_file_title")}\n${t("account.two_factor_codes_file_note")}\n\n${codes.join("\n")}\n`;

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
        title={isNew ? t("account.two_factor_is_on") : t("account.two_factor_codes_replaced")}
        description={t("account.two_factor_codes_keep")}
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
          <Button
            label={isCopied ? t("common.copied") : t("common.copy")}
            variant="secondary"
            onClick={copy}
          />
          <Button label={t("common.download")} variant="secondary" onClick={download} />
        </HStack>
        <Button label={t("account.two_factor_codes_saved")} variant="primary" onClick={onDone} />
      </HStack>
    </VStack>
  );
}
