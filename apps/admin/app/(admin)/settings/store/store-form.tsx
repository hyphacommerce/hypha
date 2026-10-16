"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { startTransition, useActionState, useState } from "react";

import { type Country, countryOptions } from "@/lib/regions";

import { type StorePatch, updateStore } from "./actions";

type Store = AdminSchemas["StoreResponse"];

/** The legal address as typed: every field a string, blank for none. */
type AddressDraft = {
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

const ADDRESS_FIELDS: { key: keyof AddressDraft; label: string; isRequired?: boolean }[] = [
  { key: "company", label: "Company" },
  { key: "first_name", label: "First name" },
  { key: "last_name", label: "Last name", isRequired: true },
  { key: "line1", label: "Address", isRequired: true },
  { key: "line2", label: "Address line 2" },
  { key: "city", label: "City", isRequired: true },
  { key: "province", label: "Region or province" },
  { key: "postal_code", label: "Postal code" },
  { key: "phone", label: "Phone" },
];

function addressDraft(a: Store["legal_address"]): AddressDraft {
  return {
    first_name: a?.first_name ?? "",
    last_name: a?.last_name ?? "",
    company: a?.company ?? "",
    line1: a?.line1 ?? "",
    line2: a?.line2 ?? "",
    city: a?.city ?? "",
    province: a?.province ?? "",
    postal_code: a?.postal_code ?? "",
    country: a?.country ?? "",
    phone: a?.phone ?? "",
  };
}

/** The address to send: `null` when every field is blank (clears it). */
function addressValue(d: AddressDraft): AdminSchemas["PostalAddressRequest"] | null {
  if (Object.values(d).every((v) => v.trim() === "")) return null;
  const optional = (v: string) => (v.trim() === "" ? null : v);
  return {
    first_name: optional(d.first_name),
    last_name: d.last_name,
    company: optional(d.company),
    line1: d.line1,
    line2: optional(d.line2),
    city: d.city,
    province: optional(d.province),
    postal_code: optional(d.postal_code),
    country: d.country,
    phone: optional(d.phone),
  };
}

/** Every IANA name the browser knows, plus the stored one if it isn't among them. */
function timezoneOptions(current: string | null | undefined) {
  const names = Intl.supportedValuesOf("timeZone");
  if (current && !names.includes(current)) names.push(current);
  return [{ value: "", label: "None" }, ...names.map((name) => ({ value: name, label: name.replaceAll("_", " ") }))];
}

/** Blank text is `null` (clears the field). */
function orNull(value: string): string | null {
  return value.trim() === "" ? null : value;
}

export function StoreForm({ store, countries }: { store: Store; countries: Country[] }) {
  const [name, setName] = useState(store.name);
  const [email, setEmail] = useState(store.contact_email ?? "");
  const [description, setDescription] = useState(store.description ?? "");
  const [legalName, setLegalName] = useState(store.legal_name ?? "");
  const [address, setAddress] = useState(() => addressDraft(store.legal_address));
  const [timezone, setTimezone] = useState(store.timezone ?? "");
  const [language, setLanguage] = useState(store.default_language);
  const [state, save, isPending] = useActionState(updateStore, {});

  // Only what changed goes in the patch, so a save never rewrites what it didn't touch.
  const patch: StorePatch = {};
  if (name.trim() !== store.name) patch.name = name;
  if (email.trim() !== (store.contact_email ?? "")) patch.contact_email = orNull(email);
  if (description.trim() !== (store.description ?? "")) patch.description = orNull(description);
  if (legalName.trim() !== (store.legal_name ?? "")) patch.legal_name = orNull(legalName);
  const savedAddress = JSON.stringify(addressValue(addressDraft(store.legal_address)));
  if (JSON.stringify(addressValue(address)) !== savedAddress) patch.legal_address = addressValue(address);
  if (timezone !== (store.timezone ?? "")) patch.timezone = orNull(timezone);
  if (language.trim() !== store.default_language) patch.default_language = language;
  const isChanged = Object.keys(patch).length > 0;

  const submit = () => {
    if (!isChanged || !name.trim()) return;
    startTransition(() => save(patch));
  };
  const field = (key: keyof AddressDraft) => (value: string) => setAddress({ ...address, [key]: value });

  return (
    <VStack gap={4}>
      {state.error && <Banner status="error" title={state.error} />}
      {state.saved && !isChanged && <Banner status="success" title="Saved." />}
      <Card>
        <VStack gap={4}>
          <Text type="display-3" as="h2">
            Store
          </Text>
          <TextInput
            label="Store name"
            description="Shown to staff here and to shoppers in the storefront. Up to 100 characters."
            value={name}
            onChange={setName}
          />
          <TextInput
            label="Contact email"
            description="Where shoppers and agents can write to you. Storefronts show it."
            placeholder="hello@example.com"
            value={email}
            onChange={setEmail}
          />
          <TextArea
            label="Description"
            description="Plain text, for your storefront's header or search results."
            maxLength={500}
            rows={3}
            value={description}
            onChange={setDescription}
          />
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Language and time
            </Text>
            <Text type="supporting" color="secondary">
              Translations come later; the language says what your content is written in now.
            </Text>
          </VStack>
          <TextInput
            label="Content language"
            description="A BCP 47 tag: en, en-GB, sr-Latn…"
            value={language}
            onChange={setLanguage}
          />
          <Selector
            label="Time zone"
            description="Times are stored in UTC; this is how your store's clients show them."
            options={timezoneOptions(store.timezone)}
            value={timezone}
            onChange={setTimezone}
            hasSearch
          />
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Legal details
            </Text>
            <Text type="supporting" color="secondary">
              For invoices and legal pages. Leave every address field blank to have none.
            </Text>
          </VStack>
          <TextInput label="Legal name" placeholder="Acme d.o.o." value={legalName} onChange={setLegalName} />
          {ADDRESS_FIELDS.map((f) => (
            <TextInput
              key={f.key}
              label={f.label}
              isOptional={!f.isRequired}
              value={address[f.key]}
              onChange={field(f.key)}
            />
          ))}
          <Selector
            label="Country"
            options={countryOptions(countries)}
            value={address.country || undefined}
            onChange={field("country")}
            hasSearch
            placeholder="Choose a country"
          />
        </VStack>
      </Card>
      <HStack gap={2} hAlign="end">
        <Button
          label="Save"
          variant="primary"
          isLoading={isPending}
          isDisabled={!isChanged || !name.trim()}
          onClick={submit}
        />
      </HStack>
    </VStack>
  );
}
