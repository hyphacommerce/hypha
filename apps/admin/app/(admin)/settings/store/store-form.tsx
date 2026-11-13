"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { startTransition, useActionState, useState } from "react";

import { useCurrencies } from "@/components/currencies";
import { SaveBar } from "@/components/save-bar";
import { currencyCodes } from "@/lib/format";
import { languageLabel } from "@/lib/languages";
import type { KeyIn } from "@/lib/messages";
import { type Country, countryOptions, currencyOptions } from "@/lib/regions";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

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

const ADDRESS_FIELDS: {
  key: keyof AddressDraft;
  label: KeyIn<"settings" | "common">;
  isRequired?: boolean;
}[] = [
  { key: "company", label: "common.company" },
  { key: "first_name", label: "common.first_name" },
  { key: "last_name", label: "common.last_name", isRequired: true },
  { key: "line1", label: "common.address", isRequired: true },
  { key: "line2", label: "common.address_line2" },
  { key: "city", label: "common.city", isRequired: true },
  { key: "province", label: "common.region_or_province" },
  { key: "postal_code", label: "common.postal_code" },
  { key: "phone", label: "common.phone" },
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

/** A blank field: the API clears it with `null`. */
function optional(v: string): string | null {
  return v.trim() === "" ? null : v;
}

/** The address to send: `null` when every field is blank (clears it). */
function addressValue(d: AddressDraft): AdminSchemas["PostalAddressRequest"] | null {
  if (Object.values(d).every((v) => v.trim() === "")) return null;
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
function timezoneOptions(current: string | null | undefined, none: string) {
  const names = Intl.supportedValuesOf("timeZone");
  if (current && !names.includes(current)) names.push(current);
  return [
    { value: "", label: none },
    ...names.map((name) => ({ value: name, label: name.replaceAll("_", " ") })),
  ];
}

/** Blank text is `null` (clears the field). */
function orNull(value: string): string | null {
  return value.trim() === "" ? null : value;
}

export function StoreForm({ store, countries }: { store: Store; countries: Country[] }) {
  const t = useT<"settings" | "languages">();
  const format = useFormat();
  const [name, setName] = useState(store.name);
  const [email, setEmail] = useState(store.contact_email ?? "");
  const [description, setDescription] = useState(store.description ?? "");
  const [legalName, setLegalName] = useState(store.legal_name ?? "");
  const [address, setAddress] = useState(() => addressDraft(store.legal_address));
  const [timezone, setTimezone] = useState(store.timezone ?? "");
  const [currency, setCurrency] = useState(store.currency ?? "");
  const currencies = useCurrencies();
  const [state, save, isPending] = useActionState(updateStore, {});

  // Only what changed goes in the patch, so a save never rewrites what it didn't touch.
  const patch: StorePatch = {};
  if (name.trim() !== store.name) patch.name = name;
  if (email.trim() !== (store.contact_email ?? "")) patch.contact_email = orNull(email);
  if (description.trim() !== (store.description ?? "")) patch.description = orNull(description);
  if (legalName.trim() !== (store.legal_name ?? "")) patch.legal_name = orNull(legalName);
  const savedAddress = JSON.stringify(addressValue(addressDraft(store.legal_address)));
  if (JSON.stringify(addressValue(address)) !== savedAddress)
    patch.legal_address = addressValue(address);
  if (timezone !== (store.timezone ?? "")) patch.timezone = orNull(timezone);
  if (currency !== (store.currency ?? "")) patch.currency = orNull(currency);
  const isChanged = Object.keys(patch).length > 0;

  const submit = () => {
    if (!isChanged || !name.trim()) return;
    startTransition(() => save(patch));
  };
  const discard = () => {
    setName(store.name);
    setEmail(store.contact_email ?? "");
    setDescription(store.description ?? "");
    setLegalName(store.legal_name ?? "");
    setAddress(addressDraft(store.legal_address));
    setTimezone(store.timezone ?? "");
    setCurrency(store.currency ?? "");
  };
  const field = (key: keyof AddressDraft) => (value: string) =>
    setAddress({ ...address, [key]: value });

  return (
    <VStack gap={4}>
      <SaveBar
        isDirty={isChanged}
        isPending={isPending}
        error={isChanged ? state.error : undefined}
        canSave={name.trim() !== ""}
        onSave={submit}
        onDiscard={discard}
      />
      {state.saved && !isChanged && <Banner status="success" title={t("common.saved_notice")} />}
      <Card>
        <VStack gap={4}>
          <Text type="display-3" as="h2">
            {t("settings.store_card_title")}
          </Text>
          <TextInput
            label={t("settings.store_name_label")}
            description={t("settings.store_name_description")}
            value={name}
            onChange={setName}
          />
          <TextInput
            label={t("settings.store_email_label")}
            description={t("settings.store_email_description")}
            placeholder={t("settings.store_email_placeholder")}
            value={email}
            onChange={setEmail}
          />
          <TextArea
            label={t("common.description")}
            description={t("settings.store_description_description")}
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
              {t("settings.store_language_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("languages.store_form_description")}
            </Text>
          </VStack>
          <div className="form-grid">
            <VStack gap={1}>
              <Text type="label">{t("settings.store_content_language_label")}</Text>
              <Text>{languageLabel(format, store.default_language)}</Text>
              <Text type="supporting" color="secondary">
                {t.rich("languages.store_form_hint", {
                  link: (chunks, key) => (
                    <Link key={key} href="/settings/languages">
                      {chunks}
                    </Link>
                  ),
                })}
              </Text>
            </VStack>
            <Selector
              label={t("common.time_zone")}
              description={t("settings.store_time_zone_description")}
              options={timezoneOptions(store.timezone, t("common.none"))}
              value={timezone}
              onChange={setTimezone}
              hasSearch
            />
          </div>
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("settings.store_costs_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("settings.store_costs_description")}
            </Text>
          </VStack>
          <Selector
            label={t("settings.store_base_currency_label")}
            description={t("settings.store_base_currency_description")}
            options={[
              { value: "", label: t("settings.store_base_currency_none") },
              ...currencyOptions(currencyCodes(currencies), format.locale),
            ]}
            value={currency}
            onChange={setCurrency}
            hasSearch
          />
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("settings.store_legal_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("settings.store_legal_description")}
            </Text>
          </VStack>
          <TextInput
            label={t("settings.store_legal_name_label")}
            placeholder={t("settings.store_legal_name_placeholder")}
            value={legalName}
            onChange={setLegalName}
          />
          <div className="form-grid">
            {ADDRESS_FIELDS.map((f) => (
              <TextInput
                key={f.key}
                label={t(f.label)}
                isOptional={!f.isRequired}
                value={address[f.key]}
                onChange={field(f.key)}
              />
            ))}
          </div>
          <Selector
            label={t("common.country")}
            options={countryOptions(countries, format.locale)}
            value={address.country || undefined}
            onChange={field("country")}
            hasSearch
            placeholder={t("common.choose_country")}
          />
        </VStack>
      </Card>
    </VStack>
  );
}
