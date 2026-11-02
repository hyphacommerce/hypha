"use client";

// The order's address check: what the verification app said about the shipping address,
// the warnings the order was placed with, and who marked it valid. Staff with
// `orders:write` mark it valid, or change the address before anything ships (the API
// checks the country's format and asks the verification app again).

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { AddressFields } from "@/components/address-fields";
import { DateTime } from "@/components/date-time";
import {
  type AddressFormat,
  type AddressValues,
  type FieldProblems,
  missingIn,
} from "@/lib/address";

import { changeOrderAddress, markAddressValid } from "../actions";

type Order = AdminSchemas["AdminOrderResponse"];
type Suggested = AdminSchemas["SuggestedAddressResponse"];

const VERDICTS: Record<
  string,
  { label: string; variant: "success" | "warning" | "error" | "neutral" }
> = {
  deliverable: { label: "Deliverable", variant: "success" },
  deliverable_with_fixes: { label: "Deliverable with corrections", variant: "warning" },
  missing_subpremise: { label: "Apartment or unit missing", variant: "warning" },
  ambiguous: { label: "Ambiguous", variant: "warning" },
  undeliverable: { label: "Undeliverable", variant: "error" },
  unknown: { label: "Not verified", variant: "neutral" },
};

const FLAGS: Record<string, string> = {
  po_box: "PO box",
  residential: "Residential",
  business: "Business",
  vacant: "Vacant",
};

/** The flags the provider said hold. */
function flagsOf(flags: AdminSchemas["VerificationFlags"]): string[] {
  return Object.entries(flags)
    .filter(([, on]) => on === true)
    .map(([flag]) => FLAGS[flag] ?? flag);
}

/** A suggestion as one line. */
function suggestionLine(s: Suggested): string {
  return [
    s.company,
    s.line1,
    s.line2,
    [s.postal_code, s.city].filter(Boolean).join(" "),
    s.province_name ?? s.province,
  ]
    .filter(Boolean)
    .join(", ");
}

/** Whether the address may still change: nothing shipped, not cancelled. */
export function canChangeAddress(order: Order): boolean {
  return order.fulfillment_status === "unfulfilled" && !order.cancelled_at;
}

export function AddressCheckCard({
  order,
  format,
  canWrite,
  names,
}: {
  order: Order;
  /** The order's country's form; none when it couldn't be read. */
  format?: AddressFormat;
  canWrite: boolean;
  /** Staff members' and apps' names, by user ID and app key. */
  names: Record<string, string>;
}) {
  const router = useRouter();
  const [error, setError] = useState<string>();
  const [editing, setEditing] = useState<"current" | "suggested" | null>(null);
  const [isPending, startTransition] = useTransition();
  const check = order.address_check;
  if (!order.shipping_address || !check) return null;
  const verification = check.verification;
  const verdict = verification ? (VERDICTS[verification.verdict] ?? VERDICTS.unknown) : null;
  const flags = verification ? flagsOf(verification.flags) : [];
  const changeable = canWrite && canChangeAddress(order);

  const markValid = () =>
    startTransition(async () => {
      const result = await markAddressValid(order.id);
      if (result.error) setError(result.error);
      else router.refresh();
    });

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} vAlign="center">
          <Text type="display-3" as="h2">
            Address check
          </Text>
          {order.address_review && <Badge label="Needs review" variant="warning" />}
        </HStack>
        {error && <Banner status="error" title={error} />}
        <MetadataList>
          <MetadataListItem label="Verification">
            {verdict ? (
              <Badge label={verdict.label} variant={verdict.variant} />
            ) : (
              "No verification app"
            )}
          </MetadataListItem>
          {verification?.failure && (
            <MetadataListItem label="Why not verified">{verification.failure}</MetadataListItem>
          )}
          {flags.length > 0 && (
            <MetadataListItem label="Flags">{flags.join(", ")}</MetadataListItem>
          )}
          {verification && (
            <MetadataListItem label="Checked">
              <DateTime value={verification.checked_at} />
              {verification.app ? ` by ${names[verification.app] ?? verification.app}` : ""}
            </MetadataListItem>
          )}
          {verification?.suggested && (
            <MetadataListItem label="Suggested">
              {suggestionLine(verification.suggested)}
            </MetadataListItem>
          )}
          {check.warnings.map((w) => (
            <MetadataListItem key={`${w.code} ${w.path}`} label="Warning">
              {w.message}
            </MetadataListItem>
          ))}
          {check.marked_valid && (
            <MetadataListItem label="Marked valid">
              <DateTime value={check.marked_valid.at} /> by{" "}
              {names[check.marked_valid.by] ?? check.marked_valid.by}
            </MetadataListItem>
          )}
        </MetadataList>
        {canWrite && (
          <HStack gap={2} wrap="wrap">
            {order.address_review && (
              <Button
                label="Mark as valid"
                variant="primary"
                isLoading={isPending}
                onClick={markValid}
              />
            )}
            {changeable && format && verification?.suggested && (
              <Button label="Use suggested address" onClick={() => setEditing("suggested")} />
            )}
          </HStack>
        )}
      </VStack>
      {editing && format && (
        <EditAddressDialog
          order={order}
          format={format}
          suggested={editing === "suggested" ? (verification?.suggested ?? undefined) : undefined}
          onClose={() => setEditing(null)}
        />
      )}
    </Card>
  );
}

/** "Edit address" for the "Ships to" card, while nothing has shipped. */
export function EditAddressButton({ order, format }: { order: Order; format: AddressFormat }) {
  const [open, setOpen] = useState(false);
  if (!order.shipping_address || !canChangeAddress(order)) return null;
  return (
    <>
      <Button label="Edit address" size="sm" onClick={() => setOpen(true)} />
      {open && <EditAddressDialog order={order} format={format} onClose={() => setOpen(false)} />}
    </>
  );
}

function blankless(v: string | null | undefined): string {
  return v ?? "";
}

/** A blank field: the API clears it with `null`. */
function optional(v: string): string | null {
  return v.trim() === "" ? null : v.trim();
}

function EditAddressDialog({
  order,
  format,
  suggested,
  onClose,
}: {
  order: Order;
  format: AddressFormat;
  /** Fills the form with the verification's suggestion, keeping names and phone. */
  suggested?: Suggested;
  onClose: () => void;
}) {
  const router = useRouter();
  const current = order.shipping_address;
  const [draft, setDraft] = useState<AddressValues>(() => ({
    first_name: blankless(current?.first_name),
    last_name: blankless(current?.last_name),
    company: blankless(suggested ? suggested.company : current?.company),
    line1: blankless(suggested ? suggested.line1 : current?.line1),
    line2: blankless(suggested ? suggested.line2 : current?.line2),
    city: blankless(suggested ? suggested.city : current?.city),
    province: blankless(suggested ? suggested.province : current?.province),
    postal_code: blankless(suggested ? suggested.postal_code : current?.postal_code),
    phone: blankless(current?.phone),
  }));
  const [error, setError] = useState<string>();
  const [problems, setProblems] = useState<FieldProblems>();
  const [tried, setTried] = useState(false);
  const [isPending, startTransition] = useTransition();

  const setField = (key: keyof AddressValues, value: string) => {
    setDraft((d) => ({ ...d, [key]: value }));
    setProblems((p) => (p?.[key] ? { ...p, [key]: undefined } : p));
  };
  const submit = () => {
    setTried(true);
    if (missingIn(format, draft).length > 0) return;
    startTransition(async () => {
      const result = await changeOrderAddress(order.id, {
        first_name: optional(draft.first_name),
        last_name: draft.last_name.trim(),
        company: optional(draft.company),
        line1: draft.line1.trim(),
        line2: optional(draft.line2),
        city: draft.city.trim(),
        province: optional(draft.province),
        postal_code: optional(draft.postal_code),
        country: order.country,
        phone: optional(draft.phone),
      });
      if (result.error) {
        setError(result.error);
        setProblems(result.problems);
        return;
      }
      router.refresh();
      onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={560} purpose="form">
      <DialogHeader
        title={suggested ? "Use suggested address" : "Edit shipping address"}
        subtitle="Before anything ships. Shipping isn't charged again; the address is verified again."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <AddressFields
          format={format}
          values={draft}
          onChange={setField}
          problems={problems}
          tried={tried}
          onFill={(filled, found) => {
            setDraft((d) => ({ ...d, ...filled }));
            setProblems(found);
          }}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button label="Save address" variant="primary" isLoading={isPending} onClick={submit} />
        </HStack>
      </VStack>
    </Dialog>
  );
}
