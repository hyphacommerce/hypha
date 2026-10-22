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
  const [isOpen, setIsOpen] = useState(false);
  return (
    <>
      <Button
        label="Edit"
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
  const [email, setEmail] = useState(customer.email);
  const [firstName, setFirstName] = useState(customer.first_name ?? "");
  const [lastName, setLastName] = useState(customer.last_name ?? "");
  const [phone, setPhone] = useState(customer.phone ?? "");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const submit = () => {
    if (email.trim() === "") {
      setError("A customer needs an email.");
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
        title="Edit customer"
        subtitle="What the shop knows about them. Orders keep the details they were placed with."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput label="Email" type="email" isRequired value={email} onChange={setEmail} />
        <div className="form-grid">
          <TextInput label="First name" isOptional value={firstName} onChange={setFirstName} />
          <TextInput label="Last name" isOptional value={lastName} onChange={setLastName} />
        </div>
        <TextInput label="Phone" isOptional value={phone} onChange={setPhone} onEnter={submit} />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button label="Save" variant="primary" isLoading={isPending} onClick={submit} />
        </HStack>
      </VStack>
    </Dialog>
  );
}
