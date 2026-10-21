"use client";

// The account's forms (design note 0034, §3): the profile, an address, the marketing
// boxes, privacy, signing out. Each saves through its server action, which answers with
// the page re-rendered.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import {
  type AddressInput,
  cancelErasure,
  removeAddress,
  requestErasure,
  saveAddress,
  setMarketing,
  signOut,
  signOutEverywhere,
  updateProfile,
} from "@/app/[country]/account/actions";

export function ProfileForm({
  country,
  initial,
}: {
  country: string;
  initial: { first_name: string; last_name: string; phone: string };
}) {
  const [values, setValues] = useState(initial);
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  const [pending, start] = useTransition();
  const set = (key: keyof typeof values) => (value: string) => {
    setSaved(false);
    setValues((v) => ({ ...v, [key]: value }));
  };
  return (
    <VStack gap={4} maxWidth={560}>
      <Grid columns={{ minWidth: 220 }} gap={3}>
        <TextInput
          size="lg"
          label="First name"
          isOptional
          value={values.first_name}
          onChange={set("first_name")}
        />
        <TextInput
          size="lg"
          label="Last name"
          isOptional
          value={values.last_name}
          onChange={set("last_name")}
        />
      </Grid>
      <TextInput size="lg" label="Phone" isOptional value={values.phone} onChange={set("phone")} />
      {error && <Banner status="error" title={error} />}
      {saved && <Banner status="success" title="Saved" />}
      <div>
        <Button
          label="Save"
          variant="primary"
          size="lg"
          isLoading={pending}
          clickAction={() =>
            start(async () => {
              const result = await updateProfile(country, values);
              setError(result?.error);
              setSaved(!result);
            })
          }
        />
      </div>
    </VStack>
  );
}

export type AddressFields = {
  first_name: string;
  last_name: string;
  company: string;
  line1: string;
  line2: string;
  postal_code: string;
  city: string;
  province: string;
  phone: string;
  default_shipping: boolean;
  default_billing: boolean;
};

const EMPTY: AddressFields = {
  first_name: "",
  last_name: "",
  company: "",
  line1: "",
  line2: "",
  postal_code: "",
  city: "",
  province: "",
  phone: "",
  default_shipping: false,
  default_billing: false,
};

const REQUIRED = ["last_name", "line1", "city"] as const;

/** `value` trimmed, or `null` for a blank optional field. */
function optional(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

/** Adds an address or edits `address`; `onDone` closes the form. */
function AddressForm({
  country,
  countryName,
  address,
  initial = EMPTY,
  onDone,
}: {
  country: string;
  countryName: string;
  /** The address being replaced; `null` for a new one. */
  address: string | null;
  initial?: AddressFields;
  onDone: () => void;
}) {
  const [values, setValues] = useState(initial);
  const [error, setError] = useState<string>();
  const [tried, setTried] = useState(false);
  const [pending, start] = useTransition();
  const set = (key: keyof AddressFields) => (value: string) =>
    setValues((v) => ({ ...v, [key]: value }));
  const required = (key: (typeof REQUIRED)[number]) =>
    tried && values[key].trim() === ""
      ? { type: "error" as const, message: "Required" }
      : undefined;
  const submit = () => {
    setTried(true);
    if (REQUIRED.some((k) => values[k].trim() === "")) return;
    const body: AddressInput = {
      first_name: optional(values.first_name),
      last_name: values.last_name.trim(),
      company: optional(values.company),
      line1: values.line1.trim(),
      line2: optional(values.line2),
      city: values.city.trim(),
      province: optional(values.province),
      postal_code: optional(values.postal_code),
      country,
      phone: optional(values.phone),
      default_shipping: values.default_shipping,
      default_billing: values.default_billing,
    };
    start(async () => {
      const result = await saveAddress(country, address, body);
      setError(result?.error);
      if (!result) onDone();
    });
  };
  return (
    <Card padding={5}>
      <VStack gap={4}>
        <Text type="large" weight="bold">
          {address ? "Edit address" : "New address"}
        </Text>
        <Text type="supporting">
          In {countryName}. To ship elsewhere, switch the currency at the top.
        </Text>
        <Grid columns={{ minWidth: 220 }} gap={3}>
          <TextInput
            label="First name"
            isOptional
            value={values.first_name}
            onChange={set("first_name")}
          />
          <TextInput
            label="Last name"
            isRequired
            value={values.last_name}
            onChange={set("last_name")}
            status={required("last_name")}
          />
        </Grid>
        <TextInput label="Company" isOptional value={values.company} onChange={set("company")} />
        <TextInput
          label="Address"
          isRequired
          value={values.line1}
          onChange={set("line1")}
          status={required("line1")}
        />
        <TextInput
          label="Apartment, suite"
          isOptional
          value={values.line2}
          onChange={set("line2")}
        />
        <Grid columns={{ minWidth: 220 }} gap={3}>
          <TextInput
            label="Postal code"
            isOptional
            value={values.postal_code}
            onChange={set("postal_code")}
          />
          <TextInput
            label="City"
            isRequired
            value={values.city}
            onChange={set("city")}
            status={required("city")}
          />
        </Grid>
        <Grid columns={{ minWidth: 220 }} gap={3}>
          <TextInput label="Region" isOptional value={values.province} onChange={set("province")} />
          <TextInput label="Phone" isOptional value={values.phone} onChange={set("phone")} />
        </Grid>
        <CheckboxInput
          label="Use as my shipping address"
          value={values.default_shipping}
          onChange={(v) => setValues((s) => ({ ...s, default_shipping: v }))}
        />
        <CheckboxInput
          label="Use as my billing address"
          value={values.default_billing}
          onChange={(v) => setValues((s) => ({ ...s, default_billing: v }))}
        />
        {error && <Banner status="error" title={error} />}
        <HStack gap={2}>
          <Button label="Save address" variant="primary" isLoading={pending} clickAction={submit} />
          <Button label="Cancel" variant="ghost" clickAction={onDone} />
        </HStack>
      </VStack>
    </Card>
  );
}

/** The address book's "new address" form, as `editing` names it. */
const NEW = "new";

export type SavedAddress = {
  id: string;
  lines: string[];
  isShipping: boolean;
  isBilling: boolean;
  fields: AddressFields;
};

/** The address book: each address with edit and remove, and a form for a new one. */
export function AddressBook({
  country,
  countryName,
  addresses,
}: {
  country: string;
  countryName: string;
  addresses: SavedAddress[];
}) {
  // An address's ID being edited, `NEW` for a new one, or nothing.
  const [editing, setEditing] = useState<string | null>(null);
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();
  return (
    <VStack gap={4} maxWidth={760}>
      {addresses.length === 0 && editing !== NEW && (
        <Text color="secondary">No saved addresses yet. Add one to check out faster.</Text>
      )}
      <Grid columns={{ minWidth: 260, max: 2 }} gap={4}>
        {addresses.map((a) =>
          editing === a.id ? (
            <AddressForm
              key={a.id}
              country={country}
              countryName={countryName}
              address={a.id}
              initial={a.fields}
              onDone={() => setEditing(null)}
            />
          ) : (
            <Card key={a.id} padding={5}>
              <VStack gap={3}>
                <VStack gap={0}>
                  {a.lines.map((line) => (
                    <Text key={line}>{line}</Text>
                  ))}
                </VStack>
                {(a.isShipping || a.isBilling) && (
                  <Text type="supporting" color="secondary">
                    {[a.isShipping && "Shipping address", a.isBilling && "Billing address"]
                      .filter(Boolean)
                      .join(" · ")}
                  </Text>
                )}
                <HStack gap={2}>
                  <Button
                    label="Edit"
                    variant="secondary"
                    size="sm"
                    clickAction={() => setEditing(a.id)}
                  />
                  <Button
                    label="Remove"
                    variant="ghost"
                    size="sm"
                    isLoading={pending}
                    clickAction={() =>
                      start(async () => {
                        const result = await removeAddress(country, a.id);
                        setError(result?.error);
                      })
                    }
                  />
                </HStack>
              </VStack>
            </Card>
          ),
        )}
      </Grid>
      {editing === NEW && (
        <AddressForm
          country={country}
          countryName={countryName}
          address={null}
          onDone={() => setEditing(null)}
        />
      )}
      {error && <Banner status="error" title={error} />}
      {editing !== NEW && (
        <div>
          <Button label="Add an address" variant="primary" clickAction={() => setEditing(NEW)} />
        </div>
      )}
    </VStack>
  );
}

/** "Email me with news and offers" and the SMS one, saved as they're ticked. */
export function MarketingChoices({
  country,
  email,
  sms,
  hasPhone,
}: {
  country: string;
  email: boolean;
  sms: boolean;
  hasPhone: boolean;
}) {
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();
  const change = (channel: "email" | "sms") => (value: boolean) =>
    start(async () => {
      const result = await setMarketing(country, channel, value);
      setError(result?.error);
    });
  return (
    <VStack gap={3} maxWidth={560}>
      <CheckboxInput
        label="Email me with news and offers"
        value={email}
        onChange={change("email")}
        isDisabled={pending}
      />
      <CheckboxInput
        label="Text me with news and offers"
        value={sms}
        onChange={change("sms")}
        isDisabled={pending || !hasPhone}
      />
      {!hasPhone && (
        <Text type="supporting" color="secondary">
          Add a phone number to your profile to get texts.
        </Text>
      )}
      {error && <Banner status="error" title={error} />}
      <Text type="supporting" color="secondary">
        You can change your mind any time; we only send what you asked for.
      </Text>
    </VStack>
  );
}

/** Signing out, here or everywhere. */
export function SignOutButtons({ country }: { country: string }) {
  const [pending, start] = useTransition();
  return (
    <HStack gap={2} wrap="wrap">
      <Button
        label="Sign out"
        variant="secondary"
        isLoading={pending}
        clickAction={() => start(() => signOut(country))}
      />
      <Button
        label="Sign out everywhere"
        variant="ghost"
        isLoading={pending}
        clickAction={() => start(() => signOutEverywhere(country))}
      />
    </HStack>
  );
}

/** Asking for the account's data to be erased, and taking it back while it's pending. */
export function ErasureControls({
  country,
  dueAt,
}: {
  country: string;
  /** When a requested erasure runs, formatted; `null` without a request. */
  dueAt: string | null;
}) {
  const [error, setError] = useState<string>();
  const [confirming, setConfirming] = useState(false);
  const [pending, start] = useTransition();
  if (dueAt) {
    return (
      <VStack gap={3}>
        <Banner
          status="warning"
          title={`Your data will be erased on ${dueAt}.`}
          description="Until then you can cancel. Afterwards your account is closed and nothing that identifies you stays with your orders."
        />
        {error && <Banner status="error" title={error} />}
        <div>
          <Button
            label="Keep my account"
            variant="primary"
            isLoading={pending}
            clickAction={() =>
              start(async () => {
                const result = await cancelErasure(country);
                setError(result?.error);
              })
            }
          />
        </div>
      </VStack>
    );
  }
  return (
    <VStack gap={3}>
      <Text color="secondary">
        We&apos;ll remove your name, contact details, addresses and marketing choices from your
        account and your orders after a short cancel window, and close your account. Your orders
        stay on record without anything that identifies you.
      </Text>
      {error && <Banner status="error" title={error} />}
      {confirming ? (
        <HStack gap={2}>
          <Button
            label="Yes, erase my data"
            variant="primary"
            isLoading={pending}
            clickAction={() =>
              start(async () => {
                const result = await requestErasure(country);
                setError(result?.error);
                setConfirming(false);
              })
            }
          />
          <Button label="Cancel" variant="ghost" clickAction={() => setConfirming(false)} />
        </HStack>
      ) : (
        <div>
          <Button
            label="Erase my data"
            variant="secondary"
            clickAction={() => setConfirming(true)}
          />
        </div>
      )}
    </VStack>
  );
}
