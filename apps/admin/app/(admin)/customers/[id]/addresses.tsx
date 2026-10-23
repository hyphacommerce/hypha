"use client";

// The customer's address book: each address as it would be written on a parcel, with its
// defaults; staff with `customers:write` add, edit and delete them. Saving sends the
// address whole (PUT), defaults included.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { PencilSquareIcon, PlusIcon, TrashIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { type Country, countryName, countryOptions } from "@/lib/regions";

import { type AddressRequest, removeAddress, saveAddress } from "../actions";

type Address = AdminSchemas["AddressResponse"];

/** An address as it would be written on a parcel. */
export function AddressLines({ address, country }: { address: Address; country: string }) {
  // Each line by what it holds, which is also its key; blank ones are left out.
  const lines = Object.entries({
    name: [address.first_name, address.last_name].filter(Boolean).join(" "),
    company: address.company,
    line1: address.line1,
    line2: address.line2,
    city: [address.postal_code, address.city].filter(Boolean).join(" "),
    province: address.province,
    country,
    phone: address.phone,
  }).filter(([, line]) => line);
  return (
    <VStack gap={0.5}>
      {lines.map(([part, line], i) => (
        <Text key={part} color={i === 0 ? "primary" : "secondary"}>
          {line}
        </Text>
      ))}
    </VStack>
  );
}

export function AddressesCard({
  customer,
  addresses,
  defaultShipping,
  defaultBilling,
  countries,
  canWrite,
}: {
  customer: string;
  addresses: Address[];
  defaultShipping: string | null | undefined;
  defaultBilling: string | null | undefined;
  countries: Country[];
  /** `customers:write`. */
  canWrite: boolean;
}) {
  const [editing, setEditing] = useState<Address | "new">();
  const [deleting, setDeleting] = useState<Address>();
  return (
    <Card>
      <VStack gap={3}>
        <HStack hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            Addresses
          </Text>
          {canWrite && (
            <Button
              label="Add address"
              variant="secondary"
              size="sm"
              icon={<PlusIcon />}
              onClick={() => setEditing("new")}
            />
          )}
        </HStack>
        {addresses.length === 0 ? (
          <Text color="secondary">
            No saved addresses. Checkout keeps an order&apos;s address on the order.
          </Text>
        ) : (
          <VStack gap={4}>
            {addresses.map((address) => (
              <HStack key={address.id} gap={2} hAlign="between" vAlign="start">
                <VStack gap={2}>
                  {(defaultShipping === address.id || defaultBilling === address.id) && (
                    <HStack gap={1}>
                      {defaultShipping === address.id && <Badge label="Default shipping" />}
                      {defaultBilling === address.id && <Badge label="Default billing" />}
                    </HStack>
                  )}
                  <AddressLines
                    address={address}
                    country={countryName(address.country, countries)}
                  />
                </VStack>
                {canWrite && (
                  <HStack gap={1}>
                    <IconButton
                      label="Edit address"
                      icon={<PencilSquareIcon />}
                      size="sm"
                      variant="ghost"
                      onClick={() => setEditing(address)}
                    />
                    <IconButton
                      label="Delete address"
                      icon={<TrashIcon />}
                      size="sm"
                      variant="ghost"
                      onClick={() => setDeleting(address)}
                    />
                  </HStack>
                )}
              </HStack>
            ))}
          </VStack>
        )}
      </VStack>
      {editing && (
        <AddressDialog
          customer={customer}
          address={editing === "new" ? undefined : editing}
          isDefaultShipping={editing !== "new" && defaultShipping === editing.id}
          isDefaultBilling={editing !== "new" && defaultBilling === editing.id}
          countries={countries}
          onClose={() => setEditing(undefined)}
        />
      )}
      {deleting && (
        <DeleteAddressDialog
          customer={customer}
          address={deleting}
          onClose={() => setDeleting(undefined)}
        />
      )}
    </Card>
  );
}

/** The address as typed: every field a string, blank for none. */
type Draft = {
  first_name: string;
  last_name: string;
  company: string;
  line1: string;
  line2: string;
  city: string;
  province: string;
  postal_code: string;
  country: string;
  phone: string;
};

const FIELDS: { key: Exclude<keyof Draft, "country">; label: string; isRequired?: boolean }[] = [
  { key: "first_name", label: "First name" },
  { key: "last_name", label: "Last name", isRequired: true },
  { key: "company", label: "Company" },
  { key: "phone", label: "Phone" },
  { key: "line1", label: "Address", isRequired: true },
  { key: "line2", label: "Address line 2" },
  { key: "postal_code", label: "Postal code" },
  { key: "city", label: "City", isRequired: true },
  { key: "province", label: "Region or province" },
];

function draftOf(a: Address | undefined, country: string): Draft {
  return {
    first_name: a?.first_name ?? "",
    last_name: a?.last_name ?? "",
    company: a?.company ?? "",
    line1: a?.line1 ?? "",
    line2: a?.line2 ?? "",
    city: a?.city ?? "",
    province: a?.province ?? "",
    postal_code: a?.postal_code ?? "",
    country: a?.country ?? country,
    phone: a?.phone ?? "",
  };
}

/** A blank field: the API clears it with `null`. */
function optional(v: string): string | null {
  return v.trim() === "" ? null : v.trim();
}

function AddressDialog({
  customer,
  address,
  isDefaultShipping,
  isDefaultBilling,
  countries,
  onClose,
}: {
  customer: string;
  /** The address to change; none adds one. */
  address?: Address;
  isDefaultShipping: boolean;
  isDefaultBilling: boolean;
  countries: Country[];
  onClose: () => void;
}) {
  const router = useRouter();
  // No country until chosen: the first of the list is nobody's default.
  const [draft, setDraft] = useState(() => draftOf(address, ""));
  const [defaultShipping, setDefaultShipping] = useState(isDefaultShipping);
  const [defaultBilling, setDefaultBilling] = useState(isDefaultBilling);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const field = (key: keyof Draft) => (value: string) => setDraft((d) => ({ ...d, [key]: value }));

  const submit = () => {
    const missing = FIELDS.filter((f) => f.isRequired && draft[f.key].trim() === "");
    if (missing.length > 0 || draft.country === "") {
      setError(
        `Needed: ${[...missing.map((f) => f.label.toLowerCase()), ...(draft.country === "" ? ["country"] : [])].join(", ")}.`,
      );
      return;
    }
    const body: AddressRequest = {
      first_name: optional(draft.first_name),
      last_name: draft.last_name.trim(),
      company: optional(draft.company),
      line1: draft.line1.trim(),
      line2: optional(draft.line2),
      city: draft.city.trim(),
      province: optional(draft.province),
      postal_code: optional(draft.postal_code),
      country: draft.country,
      phone: optional(draft.phone),
      default_shipping: defaultShipping,
      default_billing: defaultBilling,
    };
    startTransition(async () => {
      const result = await saveAddress(customer, address?.id ?? null, body);
      if (result.error) {
        setError(result.error);
        return;
      }
      router.refresh();
      onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={560} purpose="form">
      <DialogHeader
        title={address ? "Edit address" : "Add address"}
        subtitle="For the address book. Orders keep the address they were placed with."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <div className="form-grid">
          {FIELDS.map((f) => (
            <TextInput
              key={f.key}
              label={f.label}
              isOptional={!f.isRequired}
              value={draft[f.key]}
              onChange={field(f.key)}
            />
          ))}
        </div>
        <Selector
          label="Country"
          options={countryOptions(countries)}
          value={draft.country || undefined}
          onChange={field("country")}
          hasSearch
          placeholder="Choose a country"
        />
        <VStack gap={2}>
          <CheckboxInput
            label="Default shipping address"
            value={defaultShipping}
            onChange={setDefaultShipping}
          />
          <CheckboxInput
            label="Default billing address"
            value={defaultBilling}
            onChange={setDefaultBilling}
          />
        </VStack>
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={address ? "Save" : "Add address"}
            variant="primary"
            isLoading={isPending}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

function DeleteAddressDialog({
  customer,
  address,
  onClose,
}: {
  customer: string;
  address: Address;
  onClose: () => void;
}) {
  const router = useRouter();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(open) => !open && onClose()}
      title="Delete this address?"
      description={
        error ??
        `${[address.line1, address.city].filter(Boolean).join(", ")} leaves the address book. Orders placed with it keep it.`
      }
      actionLabel="Delete address"
      isActionLoading={isPending}
      onAction={() =>
        startTransition(async () => {
          const result = await removeAddress(customer, address.id);
          if (result.error) {
            setError(result.error);
            return;
          }
          router.refresh();
          onClose();
        })
      }
    />
  );
}
