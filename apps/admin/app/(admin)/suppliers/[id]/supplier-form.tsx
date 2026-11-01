"use client";

// A supplier's details as one form saved from the save bar: who they are, who to write
// to, how they're paid, how long they take, where they are.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { SaveBar } from "@/components/save-bar";
import { currencyCodes } from "@/lib/format";
import type { PaymentTerms, Supplier } from "@/lib/purchasing";
import { type Country, countryOptions, currencyOptions } from "@/lib/regions";

import { type SupplierPatch, updateSupplier } from "../actions";

/** A contact as typed, with a key of its own: contacts have no IDs. */
type Contact = { key: string; name: string; email: string; phone: string; primary: boolean };
type TermsType = "" | PaymentTerms["type"];

/** The address as typed: every field a string, blank for none. */
type AddressDraft = {
  company: string;
  first_name: string;
  last_name: string;
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
  { key: "last_name", label: "Attention of (last name)", isRequired: true },
  { key: "line1", label: "Address", isRequired: true },
  { key: "line2", label: "Address line 2" },
  { key: "city", label: "City", isRequired: true },
  { key: "province", label: "Region or province" },
  { key: "postal_code", label: "Postal code" },
  { key: "phone", label: "Phone" },
];

const MAX_CONTACTS = 10;

function addressDraft(a: Supplier["address"]): AddressDraft {
  return {
    company: a?.company ?? "",
    first_name: a?.first_name ?? "",
    last_name: a?.last_name ?? "",
    line1: a?.line1 ?? "",
    line2: a?.line2 ?? "",
    city: a?.city ?? "",
    province: a?.province ?? "",
    postal_code: a?.postal_code ?? "",
    country: a?.country ?? "",
    phone: a?.phone ?? "",
  };
}

/** Blank text is `null` (clears the field). */
function orNull(value: string): string | null {
  return value.trim() === "" ? null : value;
}

/** The address to send: `null` when every field is blank (clears it). */
function addressValue(d: AddressDraft): AdminSchemas["PostalAddressRequest"] | null {
  if (Object.values(d).every((v) => v.trim() === "")) return null;
  return {
    first_name: orNull(d.first_name),
    last_name: d.last_name,
    company: orNull(d.company),
    line1: d.line1,
    line2: orNull(d.line2),
    city: d.city,
    province: orNull(d.province),
    postal_code: orNull(d.postal_code),
    country: d.country,
    phone: orNull(d.phone),
  };
}

function contactsOf(s: Supplier): Contact[] {
  return s.contacts.map((c, i) => ({
    key: `saved-${i}`,
    name: c.name,
    email: c.email ?? "",
    phone: c.phone ?? "",
    primary: c.primary,
  }));
}

function contactsValue(contacts: Contact[]): AdminSchemas["SupplierContactRequest"][] {
  return contacts.map((c) => ({
    name: c.name,
    email: orNull(c.email),
    phone: orNull(c.phone),
    primary: c.primary,
  }));
}

function termsValue(type: TermsType, days: string, note: string): PaymentTerms | null {
  const n = orNull(note);
  switch (type) {
    case "":
      return null;
    case "net":
      return { type, days: Number(days), note: n };
    case "in_advance":
    case "on_receipt":
      return { type, note: n };
  }
}

/** A terms type from the selector's value; anything else is none. */
function asTermsType(value: string): TermsType {
  return value === "net" || value === "in_advance" || value === "on_receipt" ? value : "";
}

/** A whole number of days from 0 to `max`, or null for blank; undefined when invalid. */
function daysValue(input: string, max: number): number | null | undefined {
  if (input.trim() === "") return null;
  if (!/^\d+$/.test(input.trim())) return undefined;
  const n = Number(input);
  return n <= max ? n : undefined;
}

export function SupplierForm({
  supplier,
  countries,
  isReadOnly,
}: {
  supplier: Supplier;
  countries: Country[];
  isReadOnly: boolean;
}) {
  const currencies = useCurrencies();
  const terms = supplier.payment_terms;
  const [name, setName] = useState(supplier.name);
  const [currency, setCurrency] = useState(supplier.currency);
  const [contacts, setContacts] = useState(() => contactsOf(supplier));
  const [termsType, setTermsType] = useState<TermsType>(terms?.type ?? "");
  const [netDays, setNetDays] = useState(() => (terms?.type === "net" ? String(terms.days) : "30"));
  const [termsNote, setTermsNote] = useState(terms?.note ?? "");
  const [lead, setLead] = useState(() => supplier.lead_time_days?.toString() ?? "");
  const [taxNumber, setTaxNumber] = useState(supplier.tax_number ?? "");
  const [notes, setNotes] = useState(supplier.notes ?? "");
  const [archived, setArchived] = useState(supplier.archived);
  const [address, setAddress] = useState(() => addressDraft(supplier.address));
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  const [isPending, startTransition] = useTransition();

  const leadDays = daysValue(lead, 365);
  const days = daysValue(netDays, 365);
  const isNetValid = termsType !== "net" || (typeof days === "number" && days >= 1);
  const isValid =
    name.trim() !== "" &&
    leadDays !== undefined &&
    isNetValid &&
    contacts.every((c) => c.name.trim());

  // Only what changed goes in the patch, so a save never rewrites what it didn't touch.
  const patch: SupplierPatch = {};
  if (name.trim() !== supplier.name) patch.name = name;
  if (currency !== supplier.currency) patch.currency = currency;
  if (
    JSON.stringify(contactsValue(contacts)) !== JSON.stringify(contactsValue(contactsOf(supplier)))
  )
    patch.contacts = contactsValue(contacts);
  const newTerms = termsValue(termsType, netDays, termsNote);
  const oldTerms = terms
    ? termsValue(terms.type, terms.type === "net" ? String(terms.days) : "", terms.note ?? "")
    : null;
  if (JSON.stringify(newTerms) !== JSON.stringify(oldTerms)) patch.payment_terms = newTerms;
  if (leadDays !== undefined && leadDays !== (supplier.lead_time_days ?? null))
    patch.lead_time_days = leadDays;
  if (taxNumber.trim() !== (supplier.tax_number ?? "")) patch.tax_number = orNull(taxNumber);
  if (notes.trim() !== (supplier.notes ?? "")) patch.notes = orNull(notes);
  if (archived !== supplier.archived) patch.archived = archived;
  if (
    JSON.stringify(addressValue(address)) !==
    JSON.stringify(addressValue(addressDraft(supplier.address)))
  )
    patch.address = addressValue(address);
  const isChanged = Object.keys(patch).length > 0;

  const save = () => {
    if (!isChanged || !isValid) return;
    startTransition(async () => {
      const result = await updateSupplier(supplier.id, patch);
      setError(result.error);
      setSaved(!result.error);
    });
  };
  const discard = () => {
    setName(supplier.name);
    setCurrency(supplier.currency);
    setContacts(contactsOf(supplier));
    setTermsType(terms?.type ?? "");
    setNetDays(terms?.type === "net" ? String(terms.days) : "30");
    setTermsNote(terms?.note ?? "");
    setLead(supplier.lead_time_days?.toString() ?? "");
    setTaxNumber(supplier.tax_number ?? "");
    setNotes(supplier.notes ?? "");
    setArchived(supplier.archived);
    setAddress(addressDraft(supplier.address));
    setError(undefined);
  };
  const setContact = (i: number, change: Partial<Contact>) =>
    setContacts(
      contacts.map((c, j) => {
        if (j === i) return { ...c, ...change };
        // One primary: choosing another takes it from this one.
        return change.primary ? { ...c, primary: false } : c;
      }),
    );
  const field = (key: keyof AddressDraft) => (value: string) =>
    setAddress({ ...address, [key]: value });

  return (
    <VStack gap={4}>
      {!isReadOnly && (
        <SaveBar
          isDirty={isChanged}
          isPending={isPending}
          error={isChanged ? error : undefined}
          canSave={isValid}
          onSave={save}
          onDiscard={discard}
        />
      )}
      {saved && !isChanged && <Banner status="success" title="Saved." />}
      <Card>
        <VStack gap={4}>
          <Text type="display-3" as="h2">
            Supplier
          </Text>
          <div className="form-grid">
            <TextInput label="Name" value={name} onChange={setName} isDisabled={isReadOnly} />
            <Selector
              label="Currency"
              description="What they invoice in. Fixed while their catalogue has prices."
              options={currencyOptions(currencyCodes(currencies))}
              value={currency}
              onChange={setCurrency}
              isDisabled={isReadOnly}
              hasSearch
            />
            <TextInput
              label="Lead time (days)"
              description="How long an order takes to arrive: the default expected date."
              isOptional
              value={lead}
              onChange={setLead}
              isDisabled={isReadOnly}
              status={
                leadDays === undefined ? { type: "error", message: "0 to 365 days" } : undefined
              }
            />
            <TextInput
              label="Tax number"
              isOptional
              value={taxNumber}
              onChange={setTaxNumber}
              isDisabled={isReadOnly}
            />
          </div>
          <TextArea
            label="Notes"
            description="For staff only."
            isOptional
            maxLength={5000}
            rows={3}
            value={notes}
            onChange={setNotes}
            isDisabled={isReadOnly}
          />
          <Switch
            label="Archived"
            description="Archived suppliers take no new purchase orders."
            value={archived}
            onChange={setArchived}
            isDisabled={isReadOnly}
          />
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Contacts
            </Text>
            <Text type="supporting" color="secondary">
              Purchase orders are emailed to the primary contact. Up to {MAX_CONTACTS}.
            </Text>
          </VStack>
          {contacts.length === 0 && (
            <Text type="supporting" color="secondary">
              No contacts yet.
            </Text>
          )}
          {contacts.map((c, i) => (
            <VStack key={c.key} gap={2}>
              <div className="form-grid">
                <TextInput
                  label="Name"
                  value={c.name}
                  onChange={(v) => setContact(i, { name: v })}
                  isDisabled={isReadOnly}
                />
                <TextInput
                  label="Email"
                  isOptional
                  value={c.email}
                  onChange={(v) => setContact(i, { email: v })}
                  isDisabled={isReadOnly}
                />
                <TextInput
                  label="Phone"
                  isOptional
                  value={c.phone}
                  onChange={(v) => setContact(i, { phone: v })}
                  isDisabled={isReadOnly}
                />
              </div>
              <HStack gap={3} vAlign="center">
                <CheckboxInput
                  label="Primary: purchase orders go here"
                  value={c.primary}
                  onChange={(on) => setContact(i, { primary: on })}
                  isDisabled={isReadOnly}
                />
                {!isReadOnly && (
                  <Button
                    label="Remove contact"
                    variant="ghost"
                    icon={<TrashIcon />}
                    onClick={() => setContacts(contacts.filter((_, j) => j !== i))}
                  />
                )}
              </HStack>
            </VStack>
          ))}
          {!isReadOnly && contacts.length < MAX_CONTACTS && (
            <HStack>
              <Button
                label="Add contact"
                variant="secondary"
                onClick={() =>
                  setContacts([
                    ...contacts,
                    {
                      key: crypto.randomUUID(),
                      name: "",
                      email: "",
                      phone: "",
                      primary: contacts.length === 0,
                    },
                  ])
                }
              />
            </HStack>
          )}
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <Text type="display-3" as="h2">
            Payment terms
          </Text>
          <div className="form-grid">
            <Selector
              label="When they're paid"
              options={[
                { value: "", label: "Not set" },
                { value: "in_advance", label: "In advance" },
                { value: "on_receipt", label: "On receipt" },
                { value: "net", label: "Net days after ordering" },
              ]}
              value={termsType}
              onChange={(v) => setTermsType(asTermsType(v))}
              isDisabled={isReadOnly}
            />
            {termsType === "net" && (
              <TextInput
                label="Days"
                value={netDays}
                onChange={setNetDays}
                isDisabled={isReadOnly}
                status={isNetValid ? undefined : { type: "error", message: "1 to 365 days" }}
              />
            )}
          </div>
          {termsType !== "" && (
            <TextInput
              label="Note"
              isOptional
              placeholder="2% off within 10 days"
              value={termsNote}
              onChange={setTermsNote}
              isDisabled={isReadOnly}
            />
          )}
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Address
            </Text>
            <Text type="supporting" color="secondary">
              Printed on purchase orders. Leave every field blank to have none.
            </Text>
          </VStack>
          <div className="form-grid">
            {ADDRESS_FIELDS.map((f) => (
              <TextInput
                key={f.key}
                label={f.label}
                isOptional={!f.isRequired}
                value={address[f.key]}
                onChange={field(f.key)}
                isDisabled={isReadOnly}
              />
            ))}
          </div>
          <Selector
            label="Country"
            options={countryOptions(countries)}
            value={address.country || undefined}
            onChange={field("country")}
            isDisabled={isReadOnly}
            hasSearch
            placeholder="Choose a country"
          />
        </VStack>
      </Card>
    </VStack>
  );
}
