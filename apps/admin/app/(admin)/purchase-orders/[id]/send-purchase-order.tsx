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

import { type PurchaseOrder, poNumber } from "@/lib/purchasing";

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
 * "Send" for a draft (it's ordered too), "Send again" for an ordered one, "Send updated
 * PO" once it changed since it was last sent.
 */
function sendLabel(po: PurchaseOrder): string {
  if (po.status === "draft" || !po.sent_at) return "Send to supplier";
  return po.updated_at > po.sent_at ? "Send updated PO" : "Send again";
}

export function SendButton({ po, contacts }: { po: PurchaseOrder; contacts: Recipient[] }) {
  const [isOpen, setIsOpen] = useState(false);
  if (po.status !== "draft" && po.status !== "ordered") return null;
  return (
    <>
      <Button
        label={sendLabel(po)}
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
        title={`Send ${poNumber(po.number)}`}
        subtitle={
          po.status === "draft"
            ? "It's marked as ordered and emailed to the supplier."
            : po.revision > 0
              ? `Revision ${po.revision} is emailed: what the order says now.`
              : "The order is emailed again as it is now."
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label="To"
          description={
            primary ? undefined : "The supplier has no contact with an email: type an address."
          }
          value={to}
          onChange={setTo}
          isRequired
        />
        {others.length > 0 && (
          <VStack gap={1}>
            <Text type="supporting" color="secondary">
              Copy in
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
          label="Also copy"
          description="Other addresses, separated by commas."
          value={extra}
          onChange={setExtra}
        />
        <TextArea
          label="Message"
          description="Shown above the order in this email."
          value={message}
          onChange={setMessage}
          maxLength={2000}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Send"
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
