"use client";

// Emailing a purchase order to its supplier: who to (the primary contact first), who's
// copied, and a note for this send. A draft is ordered as it goes.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import type { TFor } from "@/lib/messages";
import { type PurchaseOrder, poNumber } from "@/lib/purchasing";
import { useT } from "@/lib/use-t";

import { sendPurchaseOrder } from "../actions";

/** A supplier contact who can be emailed. */
export type Recipient = { name: string; email: string; primary: boolean };

/** Addresses typed into one field: split on commas, semicolons and spaces. */
function addresses(text: string): string[] {
  return text
    .split(/[\s,;]+/)
    .map((a) => a.trim())
    .filter(Boolean);
}

/**
 * "Send to supplier" for one never sent (a draft is ordered too), "Send again" for one
 * sent as it is, "Send updated PO" once it was edited past the revision sent.
 */
function sendLabel(t: TFor<"purchasing">, po: PurchaseOrder): string {
  if (po.status === "draft" || po.sent_revision == null) return t("purchasing.send_to_supplier");
  return po.revision > po.sent_revision ? t("purchasing.send_updated") : t("purchasing.send_again");
}

export function SendButton({ po, contacts }: { po: PurchaseOrder; contacts: Recipient[] }) {
  const t = useT<"purchasing">();
  const [isOpen, setIsOpen] = useState(false);
  if (po.status !== "draft" && po.status !== "ordered") return null;
  return (
    <>
      <Button
        label={sendLabel(t, po)}
        variant={po.status === "draft" ? "primary" : "secondary"}
        onClick={() => setIsOpen(true)}
      />
      {isOpen && <SendDialog po={po} contacts={contacts} onClose={() => setIsOpen(false)} />}
    </>
  );
}

function SendDialog({
  po,
  contacts,
  onClose,
}: {
  po: PurchaseOrder;
  contacts: Recipient[];
  onClose: () => void;
}) {
  const t = useT<"purchasing">();
  const primary = contacts.find((c) => c.primary) ?? contacts[0];
  const [to, setTo] = useState(primary?.email ?? "");
  const [copied, setCopied] = useState<string[]>([]);
  const [extra, setExtra] = useState("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const others = contacts.filter((c) => c.email !== primary?.email);
  const recipients = addresses(to);

  const submit = () =>
    startTransition(async () => {
      const result = await sendPurchaseOrder(po.id, {
        to: recipients,
        cc: [...copied, ...addresses(extra)],
        message,
      });
      if (result.error) setError(result.error);
      else onClose();
    });

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560} purpose="form">
      <DialogHeader
        title={t("purchasing.send_dialog_title", { number: poNumber(po.number) })}
        subtitle={
          po.status === "draft"
            ? t("purchasing.send_subtitle_draft")
            : po.sent_revision == null
              ? t("purchasing.send_subtitle_first")
              : po.revision > po.sent_revision
                ? t("purchasing.send_subtitle_revision", { revision: po.revision })
                : t("purchasing.send_subtitle_again")
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={t("purchasing.send_to")}
          description={primary ? undefined : t("purchasing.send_to_no_contact")}
          value={to}
          onChange={setTo}
          isRequired
        />
        {others.length > 0 && (
          <VStack gap={1}>
            <Text type="supporting" color="secondary">
              {t("purchasing.send_copy_in")}
            </Text>
            {others.map((c) => (
              <CheckboxInput
                key={c.email}
                label={`${c.name} (${c.email})`}
                value={copied.includes(c.email)}
                onChange={(on) =>
                  setCopied(on ? [...copied, c.email] : copied.filter((e) => e !== c.email))
                }
              />
            ))}
          </VStack>
        )}
        <TextInput
          label={t("purchasing.send_also_copy")}
          description={t("purchasing.send_also_copy_description")}
          value={extra}
          onChange={setExtra}
        />
        <TextArea
          label={t("purchasing.send_message")}
          description={t("purchasing.send_message_description")}
          value={message}
          onChange={setMessage}
          maxLength={2000}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.send")}
            variant="primary"
            isLoading={isPending}
            isDisabled={recipients.length === 0}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
