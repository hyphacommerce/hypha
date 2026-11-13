"use client";

// The customer's own fields: email, name and phone, in a dialog from the page header.
// Blank name and phone clear them; the email can't be blank.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { PencilSquareIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { useT } from "@/lib/use-t";

import { updateCustomer } from "../actions";

type Customer = {
  id: string;
  email: string;
  first_name?: string | null;
  last_name?: string | null;
  phone?: string | null;
};

/** Blank text is `null` (clears the field). */
function orNull(value: string): string | null {
  return value.trim() === "" ? null : value.trim();
}

export function EditCustomer({ customer }: { customer: Customer }) {
  const t = useT<"customers">();
  const [isOpen, setIsOpen] = useState(false);
  return (
    <>
      <Button
        label={t("common.edit")}
        variant="secondary"
        icon={<PencilSquareIcon />}
        onClick={() => setIsOpen(true)}
      />
      {isOpen && <EditCustomerDialog customer={customer} onClose={() => setIsOpen(false)} />}
    </>
  );
}

function EditCustomerDialog({ customer, onClose }: { customer: Customer; onClose: () => void }) {
  const router = useRouter();
  const t = useT<"customers">();
  const [email, setEmail] = useState(customer.email);
  const [firstName, setFirstName] = useState(customer.first_name ?? "");
  const [lastName, setLastName] = useState(customer.last_name ?? "");
  const [phone, setPhone] = useState(customer.phone ?? "");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const submit = () => {
    if (email.trim() === "") {
      setError(t("customers.edit_needs_email"));
      return;
    }
    startTransition(async () => {
      const result = await updateCustomer(customer.id, {
        // The email is sent only when it changed: changing it is its own event.
        ...(email.trim() !== customer.email ? { email: email.trim() } : {}),
        first_name: orNull(firstName),
        last_name: orNull(lastName),
        phone: orNull(phone),
      });
      if (result.error) {
        setError(result.error);
        return;
      }
      router.refresh();
      onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={480} purpose="form">
      <DialogHeader
        title={t("customers.edit_title")}
        subtitle={t("customers.edit_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={t("common.email")}
          type="email"
          isRequired
          value={email}
          onChange={setEmail}
        />
        <div className="form-grid">
          <TextInput
            label={t("customers.edit_first_name")}
            isOptional
            value={firstName}
            onChange={setFirstName}
          />
          <TextInput
            label={t("customers.edit_last_name")}
            isOptional
            value={lastName}
            onChange={setLastName}
          />
        </div>
        <TextInput
          label={t("common.phone")}
          isOptional
          value={phone}
          onChange={setPhone}
          onEnter={submit}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.save")}
            variant="primary"
            isLoading={isPending}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
