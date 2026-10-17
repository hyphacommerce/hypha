"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CodeBlock } from "@astryxdesign/core/CodeBlock";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { List, ListItem } from "@astryxdesign/core/List";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Selector } from "@astryxdesign/core/Selector";
import { Step, Stepper } from "@astryxdesign/core/Stepper";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { CheckIcon } from "@heroicons/react/24/solid";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useRouter } from "next/navigation";
import { Fragment, type ReactNode, useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { currencyCodes } from "@/lib/format";
import { type Country, countryLabel, countryOptions, currencyOptions } from "@/lib/regions";

import type { SetupKey } from "../setup-checklist";
import { addMarket, addProduct, addShipping, createKey, publish, saveStore } from "./actions";

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
  /** Active products, newest first (up to 50). */
  products: AdminSchemas["ProductSummaryResponse"][];
  countries: Country[];
};

type StepDef = {
  label: string;
  description: string;
  isDone: (done: WizardData["done"]) => boolean;
  /** The "why" beside the form. */
  why: string[];
};

const STEPS: StepDef[] = [
  {
    label: "Store",
    description: "Name and contact email",
    isDone: (d) => d.store,
    why: [
      "Your storefront shows the store's name and the email shoppers can write to.",
      "The time zone decides how dates show; everything is stored in UTC.",
      "The language says what your content is written in. Translations come later.",
    ],
  },
  {
    label: "Market",
    description: "Where you sell, in which currency",
    isDone: (d) => d.market,
    why: [
      "A market is a group of countries that buy in one currency.",
      "Each country belongs to one market at most; add more markets for other currencies.",
      "A market sells the products in its catalog, created with it.",
    ],
  },
  {
    label: "Shipping",
    description: "A zone and a flat rate",
    isDone: (d) => d.shipping,
    why: [
      "Checkout offers the options of the zone the address is in.",
      "Start with one flat rate; tiers by weight or order value, and carrier rates, are in Settings → Shipping.",
    ],
  },
  {
    label: "First product",
    description: "Priced, stocked and active",
    isDone: (d) => d.product && d.stock,
    why: [
      "A product starts with one variant; options like size and colour are added on its page.",
      "Leave stock blank to not track it: it can always be sold.",
      "Drafts can't be sold, so the product is made active.",
    ],
  },
  {
    label: "Start selling",
    description: "Publish it and connect a storefront",
    isDone: (d) => d.publish && d.key,
    why: [
      "A sales channel is where products are sold; a storefront sells what's published to its channel.",
      "The storefront calls the API with a publishable key. It's safe in a storefront's settings, but shown only once.",
    ],
  },
];

export function SetupWizard({ data, initialStep }: { data: WizardData; initialStep?: number }) {
  const router = useRouter();
  const firstOpen = STEPS.findIndex((s) => !s.isDone(data.done));
  const [active, setActive] = useState(() =>
    initialStep !== undefined && initialStep >= 0 && initialStep < STEPS.length
      ? initialStep
      : Math.max(firstOpen, 0),
  );
  const allDone = firstOpen === -1;
  const step = STEPS[active];
  const isLast = active === STEPS.length - 1;

  /** After a save: re-read the data (the checklist moves on) and go to the next step. */
  const saved = () => {
    router.refresh();
    if (!isLast) setActive(active + 1);
  };

  let form: ReactNode;
  switch (active) {
    case 0:
      form = <StoreStep store={data.store} onSaved={saved} />;
      break;
    case 1:
      form = <MarketStep data={data} onSaved={saved} />;
      break;
    case 2:
      form = <ShippingStep data={data} onSaved={saved} goTo={setActive} />;
      break;
    case 3:
      form = <ProductStep data={data} onSaved={saved} goTo={setActive} />;
      break;
    default:
      form = <SellStep data={data} onSaved={() => router.refresh()} goTo={setActive} />;
  }

  return (
    <VStack gap={5}>
      <VStack gap={1}>
        <Text type="display-1" as="h1">
          Set up your store
        </Text>
        <Text color="secondary">
          Five steps to a storefront that can take orders. Each saves as you go; skip one
          and it stays on the home page's checklist.
        </Text>
      </VStack>
      {allDone && (
        <Banner
          status="success"
          title="Your store is ready to sell"
          description="Every step is done. You can still revisit them, or go back home."
          endContent={<Button label="Home" variant="secondary" href="/" />}
        />
      )}
      <HStack gap={5} vAlign="start" wrap="wrap">
        <VStack width={260}>
          <Card>
            <Stepper activeStep={active} orientation="vertical" onStepClick={setActive} label="Setup">
              {STEPS.map((s, i) => {
                const isDone = s.isDone(data.done);
                return (
                  <Step
                    key={s.label}
                    step={i}
                    label={s.label}
                    description={s.description}
                    status={isDone ? "success" : undefined}
                    indicator={isDone ? <CheckIcon width={14} aria-label="Done" /> : "number"}
                  />
                );
              })}
            </Stepper>
          </Card>
        </VStack>
        <VStack gap={4} width="100%" maxWidth={560}>
          <VStack gap={1}>
            <Text type="display-2" as="h2">
              {step.label}
            </Text>
            <Text color="secondary">{step.description}</Text>
          </VStack>
          {step.isDone(data.done) && (
            <Banner status="success" title="Done" description="Change or add to it below, or go on." />
          )}
          {/* Starts the form again once a save's refresh brings what it made, so its
              defaults (countries without a zone, a new market) aren't the old data's. */}
          <Fragment key={`${data.markets.length}/${data.zones.length}/${data.products.length}`}>
            {form}
          </Fragment>
          <HStack gap={2} hAlign="between">
            <Button
              label="Back"
              variant="ghost"
              isDisabled={active === 0}
              onClick={() => setActive(active - 1)}
            />
            {isLast ? (
              <Button label="Finish" variant="secondary" href="/" />
            ) : (
              <Button
                label={step.isDone(data.done) ? "Next" : "Skip for now"}
                variant="secondary"
                onClick={() => setActive(active + 1)}
              />
            )}
          </HStack>
        </VStack>
        <VStack gap={2} width={260}>
          <Text type="label" as="h2">
            Why this matters
          </Text>
          <List listStyle="disc" density="compact">
            {step.why.map((line) => (
              <ListItem
                key={line}
                label={
                  <Text type="supporting" color="secondary">
                    {line}
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

function StoreStep({
  store,
  onSaved,
}: {
  store: WizardData["store"];
  onSaved: () => void;
}) {
  const [name, setName] = useState(store.name);
  const [email, setEmail] = useState(store.contact_email ?? "");
  const [timezone, setTimezone] = useState(
    () => store.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone,
  );
  const [language, setLanguage] = useState(store.default_language);
  const { error, isPending, run } = useSave(onSaved);
  const canSave = name.trim() !== "" && email.trim() !== "" && language.trim() !== "";

  return (
    <Card>
      <VStack gap={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput label="Store name" value={name} onChange={setName} hasAutoFocus />
        <TextInput
          label="Contact email"
          description="Where shoppers can write to you. Storefronts show it."
          placeholder="hello@example.com"
          value={email}
          onChange={setEmail}
        />
        <Selector
          label="Time zone"
          options={timezoneOptions(timezone)}
          value={timezone}
          onChange={setTimezone}
          hasSearch
        />
        <TextInput
          label="Content language"
          description="A BCP 47 tag: en, en-GB, hr…"
          value={language}
          onChange={setLanguage}
        />
        <Text type="supporting" color="secondary">
          Legal details and a description are in Settings → Store.
        </Text>
        <HStack hAlign="end">
          <Button
            label="Save and continue"
            variant="primary"
            isLoading={isPending}
            isDisabled={!canSave}
            onClick={() =>
              run(() => saveStore({ name, contactEmail: email, timezone, language }))
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
  const [isAdding, setIsAdding] = useState(data.markets.length === 0);
  const [name, setName] = useState("");
  const [chosen, setChosen] = useState<string[]>([]);
  const [currency, setCurrency] = useState<string>();
  const [pricesIncludeTax, setPricesIncludeTax] = useState(true);
  const { error, isPending, run } = useSave(onSaved);
  const taken = new Set(data.markets.flatMap((m) => m.countries));
  const canSave = name.trim() !== "" && chosen.length > 0 && Boolean(currency);

  return (
    <VStack gap={4}>
      <Existing
        title="Your markets"
        lines={data.markets.map(
          (m) =>
            `${m.name}: ${m.countries.map((c) => countryLabel(c, data.countries)).join(", ")} · ${m.currency}`,
        )}
        addLabel="Add another market"
        isAdding={isAdding}
        onAdd={() => setIsAdding(true)}
      />
      {isAdding && (
        <Card>
          <VStack gap={4}>
            {error && <Banner status="error" title={error} />}
            <TextInput
              label="Name"
              placeholder="Eurozone"
              description="For staff only; shoppers see their country."
              value={name}
              onChange={setName}
            />
            <MultiSelector
              label="Countries"
              description="A country belongs to one market at most."
              options={countryOptions(data.countries.filter((c) => !taken.has(c.code)))}
              value={chosen}
              onChange={setChosen}
              hasSearch
              triggerDisplay="badges"
              maxBadges={6}
              placeholder="Choose countries"
            />
            <Selector
              label="Currency"
              description="What its prices are in. It can't change later."
              options={currencyOptions(currencyCodes(currencies))}
              value={currency}
              onChange={setCurrency}
              hasSearch
              placeholder="Choose a currency"
            />
            <Switch
              label="Prices include tax"
              description="On for most of Europe; off where tax is added at checkout, as in the US."
              value={pricesIncludeTax}
              onChange={setPricesIncludeTax}
            />
            <HStack hAlign="end">
              <Button
                label="Add market"
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
  return (
    <Banner
      status="info"
      title="Add a market first"
      description="This step uses the market's countries and currency."
      endContent={<Button label="Go to Market" variant="secondary" onClick={() => goTo(1)} />}
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
  const zoned = new Set(data.zones.flatMap((z) => z.countries));
  // The form starts from the first market with countries no zone covers yet.
  const first = data.markets.find((m) => m.countries.some((c) => !zoned.has(c))) ?? data.markets[0];
  const [isAdding, setIsAdding] = useState(data.zones.length === 0);
  const [name, setName] = useState(first ? first.name : "");
  const [chosen, setChosen] = useState<string[]>(() =>
    first ? first.countries.filter((c) => !zoned.has(c)) : [],
  );
  const [optionName, setOptionName] = useState("Standard");
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
        title="Your shipping zones"
        lines={data.zones.map(
          (z) =>
            `${z.name}: ${z.countries.join(", ")} · ${z.options.length} ${z.options.length === 1 ? "option" : "options"}`,
        )}
        addLabel="Add another zone"
        isAdding={isAdding}
        onAdd={() => setIsAdding(true)}
      />
      {isAdding && (
        <Card>
          <VStack gap={4}>
            {error && <Banner status="error" title={error} />}
            <TextInput label="Zone name" value={name} onChange={setName} />
            <MultiSelector
              label="Countries"
              description="A country is in one zone at most."
              options={countryOptions(
                data.countries.filter((c) => !zoned.has(c.code)),
              )}
              value={chosen}
              onChange={setChosen}
              hasSearch
              triggerDisplay="badges"
              maxBadges={6}
              placeholder="Choose countries"
            />
            <TextInput
              label="Option name"
              description="What shoppers pick at checkout."
              value={optionName}
              onChange={setOptionName}
            />
            <HStack gap={3}>
              <TextInput label="Flat rate" placeholder="4.90" value={price} onChange={setPrice} />
              <Selector
                label="Currency"
                options={currencyOptions(currencies)}
                value={currency}
                onChange={setCurrency}
              />
            </HStack>
            <HStack hAlign="end">
              <Button
                label="Add shipping"
                variant="primary"
                isLoading={isPending}
                isDisabled={!canSave}
                onClick={() =>
                  currency &&
                  run(() =>
                    addShipping({ name, countries: chosen, optionName, price, currency }),
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
  const [isAdding, setIsAdding] = useState(data.products.length === 0);
  const [title, setTitle] = useState("");
  const [price, setPrice] = useState("");
  const [market, setMarket] = useState(data.markets[0]?.id);
  const [stock, setStock] = useState("10");
  const [location, setLocation] = useState(data.locations[0]?.id);
  const { error, isPending, run } = useSave(onSaved);
  if (data.markets.length === 0) return <NeedsMarket goTo={goTo} />;

  const currency = data.markets.find((m) => m.id === market)?.currency;
  const canSave = title.trim() !== "" && price.trim() !== "" && Boolean(market);
  const shown = data.products.slice(0, 5);

  return (
    <VStack gap={4}>
      <Existing
        title="Your active products"
        lines={[
          ...shown.map((p) => p.title),
          ...(data.products.length > shown.length
            ? [`and ${data.products.length - shown.length} more`]
            : []),
        ]}
        addLabel="Add another product"
        isAdding={isAdding}
        onAdd={() => setIsAdding(true)}
      />
      {data.products.length > 0 && !data.done.stock && (
        <Banner
          status="warning"
          title="None of your first active products can be sold yet"
          description="Set stock on a product's page, or add one here with stock."
        />
      )}
      {isAdding && (
        <Card>
          <VStack gap={4}>
            {error && <Banner status="error" title={error} />}
            <TextInput
              label="Title"
              placeholder="Stoneware mug"
              description="Its handle (the URL) is made from it; both can change on its page."
              value={title}
              onChange={setTitle}
            />
            <HStack gap={3}>
              <TextInput
                label={currency ? `Price (${currency})` : "Price"}
                placeholder="18.50"
                value={price}
                onChange={setPrice}
              />
              <Selector
                label="Market"
                description="It's added to this market's catalog."
                options={data.markets.map((m) => ({ value: m.id, label: m.name }))}
                value={market}
                onChange={setMarket}
              />
            </HStack>
            <HStack gap={3}>
              <TextInput
                label="Stock"
                description="Blank for untracked: always sellable."
                value={stock}
                onChange={setStock}
              />
              {data.locations.length > 0 && (
                <Selector
                  label="At"
                  options={data.locations.map((l) => ({ value: l.id, label: l.name }))}
                  value={location}
                  onChange={setLocation}
                />
              )}
            </HStack>
            <HStack hAlign="end">
              <Button
                label="Add product"
                variant="primary"
                isLoading={isPending}
                isDisabled={!canSave}
                onClick={() =>
                  market &&
                  run(() =>
                    addProduct({ title, price, market, stock, location: location ?? null }),
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
  const online = data.channels.find((c) => c.kind === "online_store") ?? data.channels[0];
  const [channel, setChannel] = useState(online?.id);
  // Until one is picked, the newest product not yet on the channel, worked out on every
  // render: the list changes as the previous steps' saves come back.
  const [picked, setProduct] = useState<string>();
  const unpublished = data.products.filter((p) => !channel || !p.channels.includes(channel));
  const product = picked ?? unpublished[0]?.id ?? data.products[0]?.id;
  const [keyName, setKeyName] = useState("Storefront");
  const [token, setToken] = useState<string>();
  const published = useSave(onSaved);
  const key = useSave(onSaved);

  if (data.products.length === 0) {
    return (
      <Banner
        status="info"
        title="Add an active product first"
        endContent={<Button label="Go to First product" variant="secondary" onClick={() => goTo(3)} />}
      />
    );
  }
  const channelOptions = data.channels.map((c) => ({ value: c.id, label: c.name }));
  const productOptions = data.products.map((p) => ({
    value: p.id,
    label: channel && p.channels.includes(channel) ? `${p.title} (published)` : p.title,
  }));
  const isPublished = Boolean(
    channel && product && data.products.find((p) => p.id === product)?.channels.includes(channel),
  );

  return (
    <VStack gap={4}>
      <Card>
        <VStack gap={4}>
          <Text type="display-3" as="h3">
            Publish a product
          </Text>
          {published.error && <Banner status="error" title={published.error} />}
          <Selector label="Channel" options={channelOptions} value={channel} onChange={setChannel} />
          <Selector label="Product" options={productOptions} value={product} onChange={setProduct} hasSearch />
          <HStack hAlign="end">
            <Button
              label={isPublished ? "Published" : "Publish"}
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
              Connect a storefront
            </Text>
            <Text type="supporting" color="secondary">
              {data.done.key
                ? "You have a storefront key already; make another for a second storefront."
                : "A publishable key for the storefront that sells through this channel."}
            </Text>
          </VStack>
          {key.error && <Banner status="error" title={key.error} />}
          {token ? (
            <VStack gap={2}>
              <Banner
                status="warning"
                title="Copy it now"
                description="The key is shown only once. Put this line in apps/storefront/.dev.vars:"
              />
              <CodeBlock code={`STOREFRONT_KEY=${token}`} />
            </VStack>
          ) : (
            <>
              <TextInput label="Key name" value={keyName} onChange={setKeyName} />
              <HStack hAlign="end">
                <Button
                  label="Create storefront key"
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
