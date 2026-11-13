"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CodeBlock } from "@astryxdesign/core/CodeBlock";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { List, ListItem } from "@astryxdesign/core/List";
import { Selector } from "@astryxdesign/core/Selector";
import { Step, Stepper } from "@astryxdesign/core/Stepper";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import { CheckIcon } from "@heroicons/react/24/solid";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useRouter } from "next/navigation";
import { Fragment, type ReactNode, useEffect, useState, useTransition } from "react";

import { CountryPicker } from "@/components/country-picker";
import { useCurrencies } from "@/components/currencies";
import { currencyCodes } from "@/lib/format";
import type { KeyIn } from "@/lib/messages";
import { paymentPresets } from "@/lib/payments";
import { type Country, countryLabel, currencyOptions, takenCountries } from "@/lib/regions";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { createPaymentMethod } from "../settings/payments/actions";
import type { SetupKey } from "../setup-checklist";
import {
  addMarket,
  addProduct,
  addShipping,
  createKey,
  finishSetup,
  publish,
  rememberStep,
  saveStore,
} from "./actions";

type Market = AdminSchemas["MarketResponse"];
type Zone = AdminSchemas["ShippingZoneResponse"];

/** What the wizard shows, read on the server. */
export type WizardData = {
  /** The checklist's steps, done or not. */
  done: Record<SetupKey, boolean>;
  store: AdminSchemas["StoreResponse"];
  markets: Market[];
  zones: Zone[];
  locations: AdminSchemas["LocationResponse"][];
  channels: AdminSchemas["ChannelResponse"][];
  /** Payment methods; empty without `payments:read`. */
  paymentMethods: AdminSchemas["PaymentMethodResponse"][];
  /** Active products, newest first (up to 50). */
  products: AdminSchemas["ProductSummaryResponse"][];
  countries: Country[];
};

type StepKey = "store" | "market" | "shipping" | "payment" | "product" | "sell";

type StepDef = {
  /** What the store remembers it by (`setup.step`), and what its messages are named by. */
  key: StepKey;
  isDone: (done: WizardData["done"]) => boolean;
  /** The "why" beside the form: a message per line. */
  why: KeyIn<"setup">[];
};

const STEPS: StepDef[] = [
  {
    key: "store",
    isDone: (d) => d.store,
    why: [
      "setup.step_store_why_name",
      "setup.step_store_why_time_zone",
      "setup.step_store_why_language",
    ],
  },
  {
    key: "market",
    isDone: (d) => d.market,
    why: [
      "setup.step_market_why_group",
      "setup.step_market_why_one_market",
      "setup.step_market_why_catalog",
    ],
  },
  {
    key: "shipping",
    isDone: (d) => d.shipping,
    why: ["setup.step_shipping_why_zone", "setup.step_shipping_why_flat_rate"],
  },
  {
    key: "payment",
    isDone: (d) => d.payment,
    why: [
      "setup.step_payment_why_refused",
      "setup.step_payment_why_manual",
      "setup.step_payment_why_instructions",
    ],
  },
  {
    key: "product",
    isDone: (d) => d.product && d.stock,
    why: [
      "setup.step_product_why_variant",
      "setup.step_product_why_stock",
      "setup.step_product_why_active",
    ],
  },
  {
    key: "sell",
    isDone: (d) => d.publish && d.key,
    why: ["setup.step_sell_why_channel", "setup.step_sell_why_key"],
  },
];

/** Examples and names the wizard shows as they are in every language. */
const EMAIL_EXAMPLE = "hello@example.com";
const KEY_SETTING = "STOREFRONT_KEY";
const KEY_FILE = "apps/storefront/.dev.vars";

/**
 * Names the forms start with. They're saved as the store's content (shoppers pick the
 * option at checkout), which is in the store's language whatever the staff member's is,
 * so they aren't translated: staff type their own.
 */
const OPTION_NAME = "Standard";
const KEY_NAME = "Storefront";

export function SetupWizard({
  data,
  initialStep,
  savedStep,
}: {
  data: WizardData;
  initialStep?: number;
  /** The step the store remembers the merchant on (`setup.step`). */
  savedStep?: string | null;
}) {
  const router = useRouter();
  const t = useT<"setup">();
  const firstOpen = STEPS.findIndex((s) => !s.isDone(data.done));
  const saved = STEPS.findIndex((s) => s.key === savedStep);
  const [active, setActive] = useState(() =>
    initialStep !== undefined && initialStep >= 0 && initialStep < STEPS.length
      ? initialStep
      : saved >= 0
        ? saved
        : Math.max(firstOpen, 0),
  );
  const [finishError, setFinishError] = useState<string>();
  const [isFinishing, startFinishing] = useTransition();
  // The store remembers the step, so leaving and coming back opens it again.
  useEffect(() => {
    void rememberStep(STEPS[active].key);
  }, [active]);
  const finish = () =>
    startFinishing(async () => {
      const result = await finishSetup();
      if (result.error) return setFinishError(result.error);
      router.push("/");
    });
  const allDone = firstOpen === -1;
  const step = STEPS[active];
  const isLast = active === STEPS.length - 1;

  /** After a save: re-read the data (the checklist moves on) and go to the next step. */
  const onSaved = () => {
    router.refresh();
    if (!isLast) setActive(active + 1);
  };

  let form: ReactNode;
  switch (active) {
    case 0:
      form = <StoreStep store={data.store} onSaved={onSaved} />;
      break;
    case 1:
      form = <MarketStep data={data} onSaved={onSaved} />;
      break;
    case 2:
      form = <ShippingStep data={data} onSaved={onSaved} goTo={setActive} />;
      break;
    case 3:
      form = <PaymentStep data={data} onSaved={onSaved} />;
      break;
    case 4:
      form = <ProductStep data={data} onSaved={onSaved} goTo={setActive} />;
      break;
    default:
      form = <SellStep data={data} onSaved={() => router.refresh()} goTo={setActive} />;
  }

  return (
    <VStack gap={5}>
      <HStack gap={4} hAlign="between" vAlign="start" wrap="wrap">
        <VStack gap={1}>
          <Text type="display-1" as="h1">
            {t("setup.title")}
          </Text>
          <Text color="secondary">{t("setup.description")}</Text>
        </VStack>
        <Button label={t("setup.hide")} variant="ghost" isLoading={isFinishing} onClick={finish} />
      </HStack>
      {finishError && <Banner status="error" title={finishError} />}
      {allDone && (
        <Banner
          status="success"
          title={t("setup.all_done_title")}
          description={t("setup.all_done_description")}
          endContent={
            <Button
              label={t("setup.finish")}
              variant="primary"
              isLoading={isFinishing}
              onClick={finish}
            />
          }
        />
      )}
      <HStack gap={5} vAlign="start" wrap="wrap">
        <VStack width={260}>
          <Card>
            <Stepper
              activeStep={active}
              orientation="vertical"
              onStepClick={setActive}
              label={t("setup.stepper_label")}
            >
              {STEPS.map((s, i) => {
                const isDone = s.isDone(data.done);
                return (
                  <Step
                    key={s.key}
                    step={i}
                    label={t(`setup.step_${s.key}_label`)}
                    description={t(`setup.step_${s.key}_description`)}
                    status={isDone ? "success" : undefined}
                    indicator={
                      isDone ? <CheckIcon width={14} aria-label={t("common.done")} /> : "number"
                    }
                  />
                );
              })}
            </Stepper>
          </Card>
        </VStack>
        <VStack gap={4} width="100%" maxWidth={560}>
          <VStack gap={1}>
            <Text type="display-2" as="h2">
              {t(`setup.step_${step.key}_label`)}
            </Text>
            <Text color="secondary">{t(`setup.step_${step.key}_description`)}</Text>
          </VStack>
          {step.isDone(data.done) && (
            <Banner
              status="success"
              title={t("common.done")}
              description={t("setup.step_done_description")}
            />
          )}
          {/* Starts the form again once a save's refresh brings what it made, so its
              defaults (countries without a zone, a new market) aren't the old data's. */}
          <Fragment
            key={`${data.markets.length}/${data.zones.length}/${data.paymentMethods.length}/${data.products.length}`}
          >
            {form}
          </Fragment>
          <HStack gap={2} hAlign="between">
            <Button
              label={t("common.back")}
              variant="ghost"
              isDisabled={active === 0}
              onClick={() => setActive(active - 1)}
            />
            {isLast ? (
              <Button
                label={t("setup.finish")}
                variant="primary"
                isLoading={isFinishing}
                onClick={finish}
              />
            ) : (
              <Button
                label={step.isDone(data.done) ? t("common.next") : t("setup.skip")}
                variant="secondary"
                onClick={() => setActive(active + 1)}
              />
            )}
          </HStack>
        </VStack>
        <VStack gap={2} width={260}>
          <Text type="label" as="h2">
            {t("setup.why_title")}
          </Text>
          <List listStyle="disc" density="compact">
            {step.why.map((line) => (
              <ListItem
                key={line}
                label={
                  <Text type="supporting" color="secondary">
                    {t(line)}
                  </Text>
                }
              />
            ))}
          </List>
        </VStack>
      </HStack>
    </VStack>
  );
}

/** A step's save: runs `action`, shows its error, or calls `onSaved`. */
function useSave(onSaved: () => void) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const run = (action: () => Promise<{ error?: string }>) =>
    startTransition(async () => {
      setError(undefined);
      const result = await action();
      if (result.error) setError(result.error);
      else onSaved();
    });
  return { error, isPending, run };
}

/** Every IANA name the browser knows, plus `current` if it isn't among them. */
function timezoneOptions(current: string) {
  const names = Intl.supportedValuesOf("timeZone");
  if (current && !names.includes(current)) names.push(current);
  return names.map((name) => ({ value: name, label: name.replaceAll("_", " ") }));
}

function StoreStep({ store, onSaved }: { store: WizardData["store"]; onSaved: () => void }) {
  const [name, setName] = useState(store.name);
  const [email, setEmail] = useState(store.contact_email ?? "");
  const [timezone, setTimezone] = useState(
    () => store.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone,
  );
  const [language, setLanguage] = useState(store.default_language);
  const [currency, setCurrency] = useState(store.currency ?? "");
  const currencies = useCurrencies();
  const t = useT<"setup">();
  const format = useFormat();
  const { error, isPending, run } = useSave(onSaved);
  const canSave = name.trim() !== "" && email.trim() !== "" && language.trim() !== "";

  return (
    <Card>
      <VStack gap={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput label={t("setup.store_name")} value={name} onChange={setName} hasAutoFocus />
        <TextInput
          label={t("setup.store_email")}
          description={t("setup.store_email_description")}
          placeholder={EMAIL_EXAMPLE}
          value={email}
          onChange={setEmail}
        />
        <Selector
          label={t("common.time_zone")}
          options={timezoneOptions(timezone)}
          value={timezone}
          onChange={setTimezone}
          hasSearch
        />
        <TextInput
          label={t("setup.store_language")}
          description={t("setup.store_language_description")}
          value={language}
          onChange={setLanguage}
        />
        <Selector
          label={t("setup.store_currency")}
          description={t("setup.store_currency_description")}
          options={[
            { value: "", label: t("setup.store_currency_later") },
            ...currencyOptions(currencyCodes(currencies), format.locale),
          ]}
          value={currency}
          onChange={setCurrency}
          hasSearch
        />
        <Text type="supporting" color="secondary">
          {t("setup.store_more")}
        </Text>
        <HStack hAlign="end">
          <Button
            label={t("setup.store_save")}
            variant="primary"
            isLoading={isPending}
            isDisabled={!canSave}
            onClick={() =>
              run(() => saveStore({ name, contactEmail: email, timezone, language, currency }))
            }
          />
        </HStack>
      </VStack>
    </Card>
  );
}

/** What's there already, as short lines, with a button that reveals the form. */
function Existing({
  title,
  lines,
  addLabel,
  isAdding,
  onAdd,
}: {
  title: string;
  lines: string[];
  addLabel: string;
  isAdding: boolean;
  onAdd: () => void;
}) {
  if (lines.length === 0) return null;
  return (
    <Card>
      <VStack gap={2}>
        <Text type="label">{title}</Text>
        {lines.map((line) => (
          <Text key={line} type="supporting" color="secondary">
            {line}
          </Text>
        ))}
        {!isAdding && (
          <HStack>
            <Button label={addLabel} variant="secondary" onClick={onAdd} />
          </HStack>
        )}
      </VStack>
    </Card>
  );
}

function MarketStep({ data, onSaved }: { data: WizardData; onSaved: () => void }) {
  const currencies = useCurrencies();
  const t = useT<"setup">();
  const format = useFormat();
  const [isAdding, setIsAdding] = useState(data.markets.length === 0);
  const [name, setName] = useState("");
  const [chosen, setChosen] = useState<string[]>([]);
  const [currency, setCurrency] = useState<string>();
  const [pricesIncludeTax, setPricesIncludeTax] = useState(true);
  const { error, isPending, run } = useSave(onSaved);
  const taken = takenCountries(data.markets);
  const canSave = name.trim() !== "" && chosen.length > 0 && Boolean(currency);
  // A market selling everywhere lists its count, not two hundred names.
  const where = (codes: string[]) =>
    codes.length > 6
      ? t("common.countries_count", { count: codes.length })
      : format.list(
          codes.map((c) => countryLabel(c, data.countries, format.locale)),
          "unit",
        );

  return (
    <VStack gap={4}>
      <Existing
        title={t("setup.market_existing")}
        lines={data.markets.map((m) =>
          t("setup.market_line", {
            name: m.name,
            where: where(m.countries),
            currency: m.currency,
          }),
        )}
        addLabel={t("setup.market_add_another")}
        isAdding={isAdding}
        onAdd={() => setIsAdding(true)}
      />
      {isAdding && (
        <Card>
          <VStack gap={4}>
            {error && <Banner status="error" title={error} />}
            <TextInput
              label={t("common.name")}
              placeholder={t("setup.market_name_placeholder")}
              description={t("setup.market_name_description")}
              value={name}
              onChange={setName}
            />
            <CountryPicker
              countries={data.countries}
              taken={taken}
              value={chosen}
              onChange={setChosen}
              description={t("setup.market_countries_description")}
            />
            <Selector
              label={t("common.currency")}
              description={t("setup.market_currency_description")}
              options={currencyOptions(currencyCodes(currencies), format.locale)}
              value={currency}
              onChange={setCurrency}
              hasSearch
              placeholder={t("common.choose_currency")}
            />
            <Switch
              label={t("setup.market_prices_include_tax")}
              description={t("setup.market_prices_include_tax_description")}
              value={pricesIncludeTax}
              onChange={setPricesIncludeTax}
            />
            <HStack hAlign="end">
              <Button
                label={t("setup.market_add")}
                variant="primary"
                isLoading={isPending}
                isDisabled={!canSave}
                onClick={() =>
                  currency &&
                  run(() => addMarket({ name, countries: chosen, currency, pricesIncludeTax }))
                }
              />
            </HStack>
          </VStack>
        </Card>
      )}
    </VStack>
  );
}

/** A step that needs a market first says so, with a way back to that step. */
function NeedsMarket({ goTo }: { goTo: (step: number) => void }) {
  const t = useT<"setup">();
  return (
    <Banner
      status="info"
      title={t("setup.needs_market_title")}
      description={t("setup.needs_market_description")}
      endContent={
        <Button
          label={t("setup.needs_market_action")}
          variant="secondary"
          onClick={() => goTo(1)}
        />
      }
    />
  );
}

function ShippingStep({
  data,
  onSaved,
  goTo,
}: {
  data: WizardData;
  onSaved: () => void;
  goTo: (step: number) => void;
}) {
  const t = useT<"setup">();
  const format = useFormat();
  const zoned = new Set(data.zones.flatMap((z) => z.countries));
  // The form starts from the first market with countries no zone covers yet.
  const first = data.markets.find((m) => m.countries.some((c) => !zoned.has(c))) ?? data.markets[0];
  const [isAdding, setIsAdding] = useState(data.zones.length === 0);
  const [name, setName] = useState(first ? first.name : "");
  const [chosen, setChosen] = useState<string[]>(() =>
    first ? first.countries.filter((c) => !zoned.has(c)) : [],
  );
  const [optionName, setOptionName] = useState(OPTION_NAME);
  const [price, setPrice] = useState("");
  const [currency, setCurrency] = useState(first?.currency);
  const { error, isPending, run } = useSave(onSaved);
  if (!first) return <NeedsMarket goTo={goTo} />;

  const currencies = [...new Set(data.markets.map((m) => m.currency))];
  const canSave =
    name.trim() !== "" && chosen.length > 0 && optionName.trim() !== "" && price.trim() !== "";

  return (
    <VStack gap={4}>
      <Existing
        title={t("setup.shipping_existing")}
        lines={data.zones.map((z) =>
          t("setup.shipping_line", {
            name: z.name,
            where:
              z.countries.length > 6
                ? t("common.countries_count", { count: z.countries.length })
                : format.list(z.countries, "unit"),
            options: z.options.length,
          }),
        )}
        addLabel={t("setup.shipping_add_another")}
        isAdding={isAdding}
        onAdd={() => setIsAdding(true)}
      />
      {isAdding && (
        <Card>
          <VStack gap={4}>
            {error && <Banner status="error" title={error} />}
            <TextInput label={t("setup.shipping_zone_name")} value={name} onChange={setName} />
            <CountryPicker
              countries={data.countries}
              taken={takenCountries(data.zones)}
              value={chosen}
              onChange={setChosen}
              description={t("setup.shipping_countries_description")}
            />
            <TextInput
              label={t("setup.shipping_option_name")}
              description={t("setup.shipping_option_description")}
              value={optionName}
              onChange={setOptionName}
            />
            <HStack gap={3}>
              <TextInput
                label={t("setup.shipping_rate")}
                placeholder="4.90"
                value={price}
                onChange={setPrice}
              />
              <Selector
                label={t("common.currency")}
                options={currencyOptions(currencies, format.locale)}
                value={currency}
                onChange={setCurrency}
              />
            </HStack>
            <HStack hAlign="end">
              <Button
                label={t("setup.shipping_add")}
                variant="primary"
                isLoading={isPending}
                isDisabled={!canSave}
                onClick={() =>
                  currency &&
                  run(() => addShipping({ name, countries: chosen, optionName, price, currency }))
                }
              />
            </HStack>
          </VStack>
        </Card>
      )}
    </VStack>
  );
}

function PaymentStep({ data, onSaved }: { data: WizardData; onSaved: () => void }) {
  const t = useT<"setup" | "payments">();
  const presets = paymentPresets(t);
  const [isAdding, setIsAdding] = useState(data.paymentMethods.length === 0);
  const [presetValue, setPresetValue] = useState<string>(presets[0].value);
  const preset = presets.find((p) => p.value === presetValue) ?? presets[0];
  const [name, setName] = useState(preset.name);
  const [instructions, setInstructions] = useState("");
  const { error, isPending, run } = useSave(onSaved);

  return (
    <VStack gap={4}>
      <Existing
        title={t("setup.payment_existing")}
        lines={data.paymentMethods.map((m) =>
          m.enabled ? m.name : t("setup.payment_line_off", { name: m.name }),
        )}
        addLabel={t("setup.payment_add_another")}
        isAdding={isAdding}
        onAdd={() => setIsAdding(true)}
      />
      {isAdding && (
        <Card>
          <VStack gap={4}>
            {error && <Banner status="error" title={error} />}
            <Selector
              label={t("common.kind")}
              options={presets.map(({ value, label, description }) => ({
                value,
                label,
                description,
              }))}
              value={presetValue}
              onChange={(value) => {
                const next = presets.find((p) => p.value === value) ?? presets[0];
                if (name === preset.name) setName(next.name);
                setPresetValue(next.value);
              }}
            />
            <TextInput
              label={t("common.name")}
              description={t("setup.payment_name_description")}
              value={name}
              onChange={setName}
            />
            <TextArea
              label={t("setup.payment_instructions")}
              description={t("setup.payment_instructions_description")}
              placeholder={preset.instructionsPlaceholder}
              isOptional
              maxLength={2000}
              rows={3}
              value={instructions}
              onChange={setInstructions}
            />
            <HStack hAlign="end">
              <Button
                label={t("setup.payment_add")}
                variant="primary"
                isLoading={isPending}
                isDisabled={name.trim() === ""}
                onClick={() =>
                  run(() =>
                    createPaymentMethod({
                      name,
                      instructions,
                      markets: [],
                      channels: [],
                      enabled: true,
                    }),
                  )
                }
              />
            </HStack>
          </VStack>
        </Card>
      )}
    </VStack>
  );
}

function ProductStep({
  data,
  onSaved,
  goTo,
}: {
  data: WizardData;
  onSaved: () => void;
  goTo: (step: number) => void;
}) {
  const t = useT<"setup">();
  const [isAdding, setIsAdding] = useState(data.products.length === 0);
  const [title, setTitle] = useState("");
  const [price, setPrice] = useState("");
  const [market, setMarket] = useState(data.markets[0]?.id);
  const [track, setTrack] = useState(true);
  const [stock, setStock] = useState("10");
  const [location, setLocation] = useState(data.locations[0]?.id);
  const { error, isPending, run } = useSave(onSaved);
  if (data.markets.length === 0) return <NeedsMarket goTo={goTo} />;

  const currency = data.markets.find((m) => m.id === market)?.currency;
  const canSave =
    title.trim() !== "" &&
    price.trim() !== "" &&
    Boolean(market) &&
    (!track || stock.trim() !== "");
  const shown = data.products.slice(0, 5);

  return (
    <VStack gap={4}>
      <Existing
        title={t("setup.product_existing")}
        lines={[
          ...shown.map((p) => p.title),
          ...(data.products.length > shown.length
            ? [t("common.and_more", { count: data.products.length - shown.length })]
            : []),
        ]}
        addLabel={t("setup.product_add_another")}
        isAdding={isAdding}
        onAdd={() => setIsAdding(true)}
      />
      {data.products.length > 0 && !data.done.stock && (
        <Banner
          status="warning"
          title={t("setup.product_unsellable_title")}
          description={t("setup.product_unsellable_description")}
        />
      )}
      {isAdding && (
        <Card>
          <VStack gap={4}>
            {error && <Banner status="error" title={error} />}
            <TextInput
              label={t("common.title")}
              placeholder={t("setup.product_title_placeholder")}
              description={t("setup.product_title_description")}
              value={title}
              onChange={setTitle}
            />
            {/* Each pair has a description on both fields, so the inputs line up. */}
            <div className="form-grid">
              <TextInput
                label={currency ? t("setup.product_price_in", { currency }) : t("common.price")}
                description={t("setup.product_price_description")}
                placeholder="18.50"
                value={price}
                onChange={setPrice}
              />
              <Selector
                label={t("common.market")}
                description={t("setup.product_market_description")}
                options={data.markets.map((m) => ({ value: m.id, label: m.name }))}
                value={market}
                onChange={setMarket}
              />
            </div>
            <Switch
              label={t("setup.product_track")}
              description={track ? t("setup.product_track_on") : t("setup.product_track_off")}
              value={track}
              onChange={setTrack}
            />
            {track && (
              <div className="form-grid">
                <TextInput
                  label={t("setup.product_on_hand")}
                  description={t("setup.product_on_hand_description")}
                  value={stock}
                  onChange={setStock}
                />
                {data.locations.length > 0 && (
                  <Selector
                    label={t("setup.product_warehouse")}
                    description={t("setup.product_warehouse_description")}
                    options={data.locations.map((l) => ({ value: l.id, label: l.name }))}
                    value={location}
                    onChange={setLocation}
                  />
                )}
              </div>
            )}
            <HStack hAlign="end">
              <Button
                label={t("setup.product_add")}
                variant="primary"
                isLoading={isPending}
                isDisabled={!canSave}
                onClick={() =>
                  market &&
                  run(() =>
                    addProduct({ title, price, market, track, stock, location: location ?? null }),
                  )
                }
              />
            </HStack>
          </VStack>
        </Card>
      )}
    </VStack>
  );
}

function SellStep({
  data,
  onSaved,
  goTo,
}: {
  data: WizardData;
  onSaved: () => void;
  goTo: (step: number) => void;
}) {
  const t = useT<"setup">();
  const online = data.channels.find((c) => c.kind === "online_store") ?? data.channels[0];
  const [channel, setChannel] = useState(online?.id);
  // Until one is picked, the newest product not yet on the channel, worked out on every
  // render: the list changes as the previous steps' saves come back.
  const [picked, setProduct] = useState<string>();
  const unpublished = data.products.find((p) => !channel || !p.channels.includes(channel));
  const product = picked ?? unpublished?.id ?? data.products[0]?.id;
  const [keyName, setKeyName] = useState(KEY_NAME);
  const [token, setToken] = useState<string>();
  const published = useSave(onSaved);
  const key = useSave(onSaved);

  if (data.products.length === 0) {
    return (
      <Banner
        status="info"
        title={t("setup.needs_product_title")}
        endContent={
          <Button
            label={t("setup.needs_product_action")}
            variant="secondary"
            onClick={() => goTo(4)}
          />
        }
      />
    );
  }
  const channelOptions = data.channels.map((c) => ({ value: c.id, label: c.name }));
  const productOptions = data.products.map((p) => ({
    value: p.id,
    label:
      channel && p.channels.includes(channel)
        ? t("setup.sell_product_published", { title: p.title })
        : p.title,
  }));
  const isPublished = Boolean(
    channel && product && data.products.find((p) => p.id === product)?.channels.includes(channel),
  );

  return (
    <VStack gap={4}>
      <Card>
        <VStack gap={4}>
          <Text type="display-3" as="h3">
            {t("setup.sell_publish_title")}
          </Text>
          {published.error && <Banner status="error" title={published.error} />}
          <Selector
            label={t("common.channel")}
            options={channelOptions}
            value={channel}
            onChange={setChannel}
          />
          <Selector
            label={t("common.product")}
            options={productOptions}
            value={product}
            onChange={setProduct}
            hasSearch
          />
          <HStack hAlign="end">
            <Button
              label={isPublished ? t("setup.sell_published") : t("setup.sell_publish")}
              variant="primary"
              isLoading={published.isPending}
              isDisabled={!channel || !product || isPublished}
              onClick={() => channel && product && published.run(() => publish(channel, product))}
            />
          </HStack>
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h3">
              {t("setup.sell_connect_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {data.done.key ? t("setup.sell_connect_has_key") : t("setup.sell_connect_no_key")}
            </Text>
          </VStack>
          {key.error && <Banner status="error" title={key.error} />}
          {token ? (
            <VStack gap={2}>
              <Banner
                status="warning"
                title={t("setup.sell_key_copy_title")}
                description={t("setup.sell_key_copy_description")}
              />
              <CodeBlock code={token} title={t("setup.sell_key_title")} isWrapped />
              <Text type="supporting" color="secondary">
                {t("setup.sell_key_where", { setting: KEY_SETTING, file: KEY_FILE })}
              </Text>
            </VStack>
          ) : (
            <>
              <TextInput label={t("setup.sell_key_name")} value={keyName} onChange={setKeyName} />
              <HStack hAlign="end">
                <Button
                  label={t("setup.sell_key_create")}
                  variant="primary"
                  isLoading={key.isPending}
                  isDisabled={!channel || keyName.trim() === ""}
                  onClick={() =>
                    channel &&
                    key.run(async () => {
                      const result = await createKey(channel, keyName);
                      if (result.token) setToken(result.token);
                      return result;
                    })
                  }
                />
              </HStack>
            </>
          )}
        </VStack>
      </Card>
    </VStack>
  );
}
