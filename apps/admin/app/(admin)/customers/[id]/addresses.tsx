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
import { PencilSquareIcon, PlusIcon, TrashIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { AddressFields } from "@/components/address-fields";
import {
  type AddressFormat,
  type AddressValues,
  type FieldProblems,
  missingIn,
} from "@/lib/address";
import { type Country, countryName, countryOptions } from "@/lib/regions";

import { type AddressRequest, loadAddressFormat, removeAddress, saveAddress } from "../actions";

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
    province: address.province_name ?? address.province,
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
  formats,
  canWrite,
}: {
  customer: string;
  addresses: Address[];
  defaultShipping: string | null | undefined;
  defaultBilling: string | null | undefined;
  countries: Country[];
  /** The address forms of the countries in the book, by country. */
  formats: Record<string, AddressFormat>;
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
          initialFormat={editing === "new" ? undefined : formats[editing.country]}
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
type Draft = AddressValues & { country: string };

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
  initialFormat,
  onClose,
}: {
  customer: string;
  /** The address to change; none adds one. */
  address?: Address;
  isDefaultShipping: boolean;
  isDefaultBilling: boolean;
  countries: Country[];
  /** The address's country's form, when editing one. */
  initialFormat?: AddressFormat;
  onClose: () => void;
}) {
  const router = useRouter();
  // No country until chosen: the first of the list is nobody's default.
  const [draft, setDraft] = useState(() => draftOf(address, ""));
  // The chosen country's form: its fields, labels and regions.
  const [format, setFormat] = useState<AddressFormat | null>(initialFormat ?? null);
  const [defaultShipping, setDefaultShipping] = useState(isDefaultShipping);
  const [defaultBilling, setDefaultBilling] = useState(isDefaultBilling);
  const [error, setError] = useState<string>();
  const [problems, setProblems] = useState<FieldProblems>();
  const [tried, setTried] = useState(false);
  const [isPending, startTransition] = useTransition();
  const [isLoadingFormat, startLoading] = useTransition();

  const chooseCountry = (country: string) => {
    setDraft((d) => ({ ...d, country }));
    startLoading(async () => setFormat(await loadAddressFormat(country)));
  };
  const setField = (key: keyof AddressValues, value: string) => {
    setDraft((d) => ({ ...d, [key]: value }));
    // A field changed since the save no longer has the problem it reported.
    setProblems((p) => (p?.[key] ? { ...p, [key]: undefined } : p));
  };

  const submit = () => {
    setTried(true);
    if (draft.country === "" || !format) {
      setError("Choose a country first.");
      return;
    }
    if (missingIn(format, draft).length > 0) return;
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
        title={address ? "Edit address" : "Add address"}
        subtitle="For the address book. Orders keep the address they were placed with."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <Selector
          label="Country"
          options={countryOptions(countries)}
          value={draft.country || undefined}
          onChange={chooseCountry}
          hasSearch
          placeholder="Choose a country"
          isRequired
        />
        {format && !isLoadingFormat ? (
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
        ) : (
          draft.country !== "" && <Text color="secondary">Loading the country&apos;s form…</Text>
        )}
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
