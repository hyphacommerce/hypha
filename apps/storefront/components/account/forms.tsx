"use client";

// The account's forms: the profile, an address, the marketing boxes, privacy, signing
// out. Each saves through its server action, which answers with the page re-rendered.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import { AddressFields } from "@/components/address-fields";
import {
  type AddressFormat,
  type AddressValues,
  type FieldProblems,
  missingIn,
} from "@/lib/address";
import { useT } from "@/lib/use-t";

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
} from "@/app/[country]/(shop)/account/actions";
import { useLocale } from "@/components/locale";

/** The account's messages and the address form's, beside the shared ones: the account's
 * layout sends them. */
const useAccountT = () => useT<"account" | "address">();

export function ProfileForm({
  initial,
}: {
  initial: { first_name: string; last_name: string; phone: string };
}) {
  const locale = useLocale();
  const t = useAccountT();
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
          label={t("account.first_name")}
          isOptional
          value={values.first_name}
          onChange={set("first_name")}
        />
        <TextInput
          size="lg"
          label={t("account.last_name")}
          isOptional
          value={values.last_name}
          onChange={set("last_name")}
        />
      </Grid>
      <TextInput
        size="lg"
        label={t("account.phone")}
        isOptional
        value={values.phone}
        onChange={set("phone")}
      />
      {error && <Banner status="error" title={error} />}
      {saved && <Banner status="success" title={t("account.saved")} />}
      <div>
        <Button
          label={t("account.save")}
          variant="primary"
          size="lg"
          isLoading={pending}
          clickAction={() =>
            start(async () => {
              const result = await updateProfile(locale, values);
              setError(result?.error);
              setSaved(!result);
            })
          }
        />
      </div>
    </VStack>
  );
}

export type AddressFields = AddressValues & {
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

/** `value` trimmed, or `null` for a blank optional field. */
function optional(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

/** Adds an address or edits `address`, laid out as the country does (`format`);
 * `onDone` closes the form. */
function AddressForm({
  country,
  countryName,
  format,
  address,
  initial = EMPTY,
  onDone,
}: {
  country: string;
  countryName: string;
  format: AddressFormat;
  /** The address being replaced; `null` for a new one. */
  address: string | null;
  initial?: AddressFields;
  onDone: () => void;
}) {
  const locale = useLocale();
  const t = useAccountT();
  const [values, setValues] = useState(initial);
  const [error, setError] = useState<string>();
  const [problems, setProblems] = useState<FieldProblems>();
  const [tried, setTried] = useState(false);
  const [pending, start] = useTransition();
  const set = (key: keyof AddressValues, value: string) => {
    setValues((v) => ({ ...v, [key]: value }));
    // A field changed since the save no longer has the problem it reported.
    setProblems((p) => (p?.[key] ? { ...p, [key]: undefined } : p));
  };
  const submit = () => {
    setTried(true);
    if (missingIn(format, values).length > 0) return;
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
      const result = await saveAddress(locale, address, body);
      setError(result?.error);
      setProblems(result?.problems);
      if (!result) onDone();
    });
  };
  return (
    <Card padding={5}>
      <VStack gap={4}>
        <Text type="large" weight="bold">
          {address ? t("account.edit_address") : t("account.new_address")}
        </Text>
        <Text type="supporting">{t("account.address_country", { country: countryName })}</Text>
        <AddressFields
          format={format}
          values={values}
          onChange={set}
          problems={problems}
          tried={tried}
          onFill={(filled, found) => {
            setValues((v) => ({ ...v, ...filled }));
            setProblems(found);
          }}
        />
        <CheckboxInput
          label={t("account.default_shipping")}
          value={values.default_shipping}
          onChange={(v) => setValues((s) => ({ ...s, default_shipping: v }))}
        />
        <CheckboxInput
          label={t("account.default_billing")}
          value={values.default_billing}
          onChange={(v) => setValues((s) => ({ ...s, default_billing: v }))}
        />
        {error && <Banner status="error" title={error} />}
        <HStack gap={2}>
          <Button
            label={t("account.save_address")}
            variant="primary"
            isLoading={pending}
            clickAction={submit}
          />
          <Button label={t("common.cancel")} variant="ghost" clickAction={onDone} />
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
  format,
  addresses,
}: {
  country: string;
  countryName: string;
  /** How the country lays out its addresses. */
  format: AddressFormat;
  addresses: SavedAddress[];
}) {
  const locale = useLocale();
  const t = useAccountT();
  // An address's ID being edited, `NEW` for a new one, or nothing.
  const [editing, setEditing] = useState<string | null>(null);
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();
  return (
    <VStack gap={4} maxWidth={760}>
      {addresses.length === 0 && editing !== NEW && (
        <Text color="secondary">{t("account.no_addresses")}</Text>
      )}
      <Grid columns={{ minWidth: 260, max: 2 }} gap={4}>
        {addresses.map((a) =>
          editing === a.id ? (
            <AddressForm
              key={a.id}
              country={country}
              countryName={countryName}
              format={format}
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
                    {[
                      a.isShipping && t("account.shipping_address"),
                      a.isBilling && t("account.billing_address"),
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </Text>
                )}
                <HStack gap={2}>
                  <Button
                    label={t("account.edit")}
                    variant="secondary"
                    size="sm"
                    clickAction={() => setEditing(a.id)}
                  />
                  <Button
                    label={t("common.remove")}
                    variant="ghost"
                    size="sm"
                    isLoading={pending}
                    clickAction={() =>
                      start(async () => {
                        const result = await removeAddress(locale, a.id);
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
          format={format}
          address={null}
          onDone={() => setEditing(null)}
        />
      )}
      {error && <Banner status="error" title={error} />}
      {editing !== NEW && (
        <div>
          <Button
            label={t("account.add_address")}
            variant="primary"
            clickAction={() => setEditing(NEW)}
          />
        </div>
      )}
    </VStack>
  );
}

/** "Email me with news and offers" and the SMS one, saved as they're ticked. */
export function MarketingChoices({
  email,
  sms,
  hasPhone,
}: {
  email: boolean;
  sms: boolean;
  hasPhone: boolean;
}) {
  const locale = useLocale();
  const t = useAccountT();
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();
  const change = (channel: "email" | "sms") => (value: boolean) =>
    start(async () => {
      const result = await setMarketing(locale, channel, value);
      setError(result?.error);
    });
  return (
    <VStack gap={3} maxWidth={560}>
      <CheckboxInput
        label={t("account.email_opt_in")}
        value={email}
        onChange={change("email")}
        isDisabled={pending}
      />
      <CheckboxInput
        label={t("account.sms_opt_in")}
        value={sms}
        onChange={change("sms")}
        isDisabled={pending || !hasPhone}
      />
      {!hasPhone && (
        <Text type="supporting" color="secondary">
          {t("account.sms_needs_phone")}
        </Text>
      )}
      {error && <Banner status="error" title={error} />}
      <Text type="supporting" color="secondary">
        {t("account.change_mind")}
      </Text>
    </VStack>
  );
}

/** Signing out, here or everywhere. */
export function SignOutButtons() {
  const locale = useLocale();
  const t = useAccountT();
  const [pending, start] = useTransition();
  return (
    <HStack gap={2} wrap="wrap">
      <Button
        label={t("account.sign_out")}
        variant="secondary"
        isLoading={pending}
        clickAction={() => start(() => signOut(locale))}
      />
      <Button
        label={t("account.sign_out_everywhere")}
        variant="ghost"
        isLoading={pending}
        clickAction={() => start(() => signOutEverywhere(locale))}
      />
    </HStack>
  );
}

/** Asking for the account's data to be erased, and taking it back while it's pending. */
export function ErasureControls({
  dueAt,
}: {
  /** When a requested erasure runs, formatted; `null` without a request. */
  dueAt: string | null;
}) {
  const locale = useLocale();
  const t = useAccountT();
  const [error, setError] = useState<string>();
  const [confirming, setConfirming] = useState(false);
  const [pending, start] = useTransition();
  if (dueAt) {
    return (
      <VStack gap={3}>
        <Banner
          status="warning"
          title={t("account.erasure_due_title", { date: dueAt })}
          description={t("account.erasure_due_description")}
        />
        {error && <Banner status="error" title={error} />}
        <div>
          <Button
            label={t("account.keep_account")}
            variant="primary"
            isLoading={pending}
            clickAction={() =>
              start(async () => {
                const result = await cancelErasure(locale);
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
      <Text color="secondary">{t("account.erasure_description")}</Text>
      {error && <Banner status="error" title={error} />}
      {confirming ? (
        <HStack gap={2}>
          <Button
            label={t("account.confirm_erase")}
            variant="primary"
            isLoading={pending}
            clickAction={() =>
              start(async () => {
                const result = await requestErasure(locale);
                setError(result?.error);
                setConfirming(false);
              })
            }
          />
          <Button
            label={t("common.cancel")}
            variant="ghost"
            clickAction={() => setConfirming(false)}
          />
        </HStack>
      ) : (
        <div>
          <Button
            label={t("account.erase")}
            variant="secondary"
            clickAction={() => setConfirming(true)}
          />
        </div>
      )}
    </VStack>
  );
}
