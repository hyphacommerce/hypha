"use client";

// A price list's settings: what it's called, what kind of price it holds, when it's
// live, who it prices for, how it adjusts the base prices and, for a new list, the
// markets it goes on. The
// new list page starts from a template; a list's page edits the same form.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Link } from "@astryxdesign/core/Link";
import { Card } from "@astryxdesign/core/Card";
import { DateTimeInput, type ISODateTimeString } from "@astryxdesign/core/DateTimeInput";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { SelectableCard } from "@astryxdesign/core/SelectableCard";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Token } from "@astryxdesign/core/Token";
import { useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { useTimeZone } from "@/components/date-time";
import { ProductPicker } from "@/components/product-picker";
import { SaveBar } from "@/components/save-bar";
import { currencyCodes } from "@/lib/format";
import {
  type FormErrors,
  KINDS,
  type ListForm,
  type PriceList,
  TEMPLATES,
  type TemplateKey,
  checkForm,
  clearErrors,
  emptyForm,
  formFromList,
  parsePercent,
  percentLabel,
  toLocalInput,
} from "@/lib/price-lists";

import { createPriceList, updatePriceList } from "./actions";

export type MarketOption = { id: string; name: string; currency: string; catalog: string };

/** What the date and time input takes: a wall-clock time to the minute. */
const isMinute = (text: string): text is ISODateTimeString =>
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(text);

const status = (message: string | undefined) =>
  message ? { type: "error" as const, message } : undefined;

/** Tomorrow at midnight and a week after, in `timeZone`: a sale template's window. */
function nextWeek(timeZone: string): { startsAt: string; endsAt: string } {
  const day = (offset: number) =>
    toLocalInput(new Date(Date.now() + offset * 86_400_000).toISOString(), timeZone).slice(0, 10);
  return { startsAt: `${day(1)}T00:00`, endsAt: `${day(8)}T00:00` };
}

/** The settings cards, shared by the new list page and a list's page. */
function SettingsFields({
  form,
  errors,
  set,
  isNew,
  isReadOnly,
  categories,
  groups,
  productNames,
  onPickProducts,
}: {
  form: ListForm;
  errors: FormErrors;
  set: (change: Partial<ListForm>) => void;
  isNew: boolean;
  isReadOnly: boolean;
  categories: { value: string; label: string }[];
  /** The store's customer groups; none without `customers:read`. */
  groups: { value: string; label: string }[];
  productNames: Record<string, string>;
  onPickProducts: () => void;
}) {
  const currencies = useCurrencies();
  const timeZone = useTimeZone();
  const bps = parsePercent(form.percent);
  // A group the list names that isn't among those read (no `customers:read`) shows by ID.
  const groupOptions = [
    ...groups,
    ...form.groups
      .filter((id) => !groups.some((g) => g.value === id))
      .map((id) => ({ value: id, label: id })),
  ];

  return (
    <>
      <Card>
        <VStack gap={4}>
          <Text type="display-3" as="h2">
            Details
          </Text>
          <TextInput
            label="Name"
            description="For staff, unique: shown wherever the list is chosen."
            placeholder="Black Friday"
            value={form.name}
            onChange={(name) => set({ name })}
            isDisabled={isReadOnly}
            status={status(errors.name)}
            width="100%"
          />
          <TextArea
            label="Description"
            isOptional
            rows={2}
            maxLength={255}
            value={form.description}
            onChange={(description) => set({ description })}
            isDisabled={isReadOnly}
            status={status(errors.description)}
          />
          <Selector
            label="Currency"
            description={
              isNew
                ? "Fixed once it's created: its fixed prices are in it, and only markets in this currency can use it."
                : "Fixed: its fixed prices are in it."
            }
            options={currencyCodes(currencies)}
            value={form.currency}
            onChange={(currency) => set({ currency })}
            hasSearch
            isDisabled={!isNew || isReadOnly}
            status={status(errors.currency)}
            width={240}
          />
          <RadioList
            label="Kind"
            value={form.kind}
            onChange={(kind) => set({ kind: kind === "sale" ? "sale" : "override" })}
            isDisabled={isReadOnly}
          >
            {KINDS.map((k) => (
              <RadioListItem
                key={k.value}
                value={k.value}
                label={k.label}
                description={k.description}
              />
            ))}
          </RadioList>
          {form.kind === "sale" && (
            <Text type="supporting" color="secondary">
              Selling in the EU? During a reduction the price struck through must be the lowest of
              the last 30 days. Hypha doesn&apos;t track that yet: set the variants&apos; compare-at
              prices (or the fixed prices&apos; own) to it yourself.
            </Text>
          )}
          {form.kind === "override" && (
            <RadioList
              label="Compare-at prices"
              value={form.compareAt}
              onChange={(compareAt) =>
                set({ compareAt: compareAt === "hidden" ? "hidden" : "adjusted" })
              }
              isDisabled={isReadOnly}
            >
              <RadioListItem
                value="adjusted"
                label="Adjust them"
                description="A variant's compare-at price shows with the list's adjustment applied."
              />
              <RadioListItem
                value="hidden"
                label="Hide them"
                description="Only a fixed price's own compare-at price shows."
              />
            </RadioList>
          )}
        </VStack>
      </Card>

      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Schedule
            </Text>
            <Text type="supporting" color="secondary">
              Times are in the store&apos;s time zone, {timeZone}. Prices change at the moment it
              starts and ends; search and the storefront&apos;s pages follow within a minute.
            </Text>
          </VStack>
          <Switch
            label="Active"
            description="A draft prices nothing, whatever its dates say: switch it off to pause the list."
            value={form.status === "active"}
            onChange={(on) => set({ status: on ? "active" : "draft" })}
            isDisabled={isReadOnly}
          />
          <HStack gap={4} wrap="wrap">
            <DateTimeInput
              label="Starts"
              description="Empty: at once."
              value={isMinute(form.startsAt) ? form.startsAt : undefined}
              onChange={(value) => set({ startsAt: value ?? "" })}
              hasClear
              hourFormat="24h"
              weekStartsOn="mon"
              isDisabled={isReadOnly}
              status={status(errors.startsAt)}
            />
            <DateTimeInput
              label="Ends"
              description="Empty: never."
              value={isMinute(form.endsAt) ? form.endsAt : undefined}
              onChange={(value) => set({ endsAt: value ?? "" })}
              hasClear
              hourFormat="24h"
              weekStartsOn="mon"
              isDisabled={isReadOnly}
              status={status(errors.endsAt)}
            />
          </HStack>
        </VStack>
      </Card>

      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Audience
            </Text>
            <Text type="supporting" color="secondary">
              Who it prices for. Without a group, every shopper. With groups, only signed-in
              customers in one of them: in their carts and on product pages once they sign in.
              Listings and search show everyone&apos;s prices.
            </Text>
          </VStack>
          {groupOptions.length === 0 ? (
            <Text color="secondary">
              Everyone. To price for some customers, make a{" "}
              <Link href="/customers/groups">customer group</Link> first.
            </Text>
          ) : (
            <MultiSelector
              label="Customer groups"
              placeholder="Everyone"
              options={groupOptions}
              value={form.groups}
              onChange={(chosen) => set({ groups: chosen })}
              hasSearch
              isDisabled={isReadOnly}
              status={status(errors.groups)}
              width="100%"
            />
          )}
        </VStack>
      </Card>

      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Adjustment
            </Text>
            <Text type="supporting" color="secondary">
              A percentage of the base price, for the products it covers. Variants with a fixed
              price in the list cost that instead. Without an adjustment, the list prices only the
              variants it has fixed prices for.
            </Text>
          </VStack>
          <Switch
            label="Adjust base prices"
            value={form.hasAdjustment}
            onChange={(hasAdjustment) =>
              set({ hasAdjustment, percent: hasAdjustment && !form.percent ? "-10" : form.percent })
            }
            isDisabled={isReadOnly}
          />
          {form.hasAdjustment && (
            <>
              <TextInput
                label="Percentage"
                description={
                  bps === null
                    ? "-20 is 20% off; 5 is 5% more."
                    : bps === 0
                      ? "0: the base prices as they are."
                      : `${percentLabel(bps)}: ${bps < 0 ? "a reduction" : "a markup"} of the base price.`
                }
                value={form.percent}
                onChange={(percent) => set({ percent })}
                isDisabled={isReadOnly}
                status={status(errors.percent)}
                width={200}
              />
              <RadioList
                label="Applies to"
                value={form.scope}
                onChange={(scope) =>
                  set({
                    scope:
                      scope === "products"
                        ? "products"
                        : scope === "categories"
                          ? "categories"
                          : "all",
                  })
                }
                isDisabled={isReadOnly}
              >
                <RadioListItem value="all" label="Every product" />
                <RadioListItem
                  value="categories"
                  label="Products in categories"
                  description="A category counts the products in its subcategories too."
                />
                <RadioListItem value="products" label="Chosen products" />
              </RadioList>
              {form.scope === "categories" && (
                <VStack gap={2}>
                  <MultiSelector
                    label="Categories"
                    placeholder="Choose categories"
                    options={categories}
                    value={form.categories}
                    onChange={(chosen) => set({ categories: chosen })}
                    hasSearch
                    triggerDisplay="count"
                    formatValue={(items) =>
                      items.length === 1 ? "1 category" : `${items.length} categories`
                    }
                    isDisabled={isReadOnly}
                    status={status(errors.categories)}
                    width="100%"
                  />
                  {form.categories.length > 0 && (
                    <HStack gap={1} wrap="wrap">
                      {form.categories.map((id) => (
                        <Token
                          key={id}
                          size="sm"
                          label={categories.find((c) => c.value === id)?.label ?? id}
                          onRemove={
                            isReadOnly
                              ? undefined
                              : () => set({ categories: form.categories.filter((c) => c !== id) })
                          }
                        />
                      ))}
                    </HStack>
                  )}
                </VStack>
              )}
              {form.scope === "products" && (
                <VStack gap={2} hAlign="start">
                  {errors.products && <Banner status="error" title={errors.products} />}
                  {form.products.length > 0 && (
                    <HStack gap={1} wrap="wrap">
                      {form.products.map((id) => (
                        <Token
                          key={id}
                          size="sm"
                          label={productNames[id] ?? id}
                          onRemove={
                            isReadOnly
                              ? undefined
                              : () => set({ products: form.products.filter((p) => p !== id) })
                          }
                        />
                      ))}
                    </HStack>
                  )}
                  {!isReadOnly && (
                    <Button label="Choose products" variant="secondary" onClick={onPickProducts} />
                  )}
                </VStack>
              )}
            </>
          )}
        </VStack>
      </Card>
    </>
  );
}

/** The new list page: a template to start from, the settings, and the markets. */
export function NewPriceListForm({
  defaultCurrency,
  markets,
  preselected,
  categories,
  groups,
}: {
  defaultCurrency: string;
  /** Markets with a catalog, to put the list on. */
  markets: MarketOption[];
  /** A market to put it on from the start (`?market=`). */
  preselected?: string;
  categories: { value: string; label: string }[];
  groups: { value: string; label: string }[];
}) {
  const timeZone = useTimeZone();
  const [form, setForm] = useState<ListForm>(() => emptyForm(defaultCurrency));
  const [template, setTemplate] = useState<TemplateKey | null>(null);
  const [chosenMarkets, setChosenMarkets] = useState<string[]>(() =>
    preselected && markets.some((m) => m.id === preselected) ? [preselected] : [],
  );
  const [names, setNames] = useState<Record<string, string>>({});
  const [isPicking, setIsPicking] = useState(false);
  const [errors, setErrors] = useState<FormErrors>({});
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const set = (change: Partial<ListForm>) => {
    setForm((before) => ({ ...before, ...change }));
    setErrors((before) => clearErrors(before, change));
  };
  const choose = (key: TemplateKey) => {
    const found = TEMPLATES.find((t) => t.key === key);
    if (!found) return;
    setTemplate(key);
    setForm((before) => found.apply(before, nextWeek(timeZone)));
    setErrors({});
  };
  const sameCurrency = markets.filter((m) => m.currency === form.currency);
  const catalogs = chosenMarkets.flatMap((id) => {
    const market = sameCurrency.find((m) => m.id === id);
    return market ? [market.catalog] : [];
  });

  const submit = () => {
    const found = checkForm(form, timeZone);
    setErrors(found);
    if (Object.keys(found).length > 0) return;
    startTransition(async () => {
      const result = await createPriceList(form, catalogs);
      setErrors(result.fieldErrors ?? {});
      setError(result.error);
    });
  };

  return (
    <VStack gap={5}>
      <Card>
        <VStack gap={3}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Start from
            </Text>
            <Text type="supporting" color="secondary">
              A template fills in the settings a kind of list needs; change any of them below.
            </Text>
          </VStack>
          <div className="template-grid">
            {TEMPLATES.map((t) => (
              <SelectableCard
                key={t.key}
                label={t.label}
                isSelected={template === t.key}
                onChange={(isSelected) => isSelected && choose(t.key)}
                padding={3}
              >
                <VStack gap={1}>
                  <Text weight="semibold">{t.label}</Text>
                  <Text type="supporting" color="secondary">
                    {t.description}
                  </Text>
                </VStack>
              </SelectableCard>
            ))}
          </div>
        </VStack>
      </Card>

      {error && <Banner status="error" title={error} />}

      <SettingsFields
        form={form}
        errors={errors}
        set={set}
        isNew
        isReadOnly={false}
        categories={categories}
        groups={groups}
        productNames={names}
        onPickProducts={() => setIsPicking(true)}
      />

      <Card>
        <VStack gap={3}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Markets
            </Text>
            <Text type="supporting" color="secondary">
              Where it prices: the markets whose catalogs use it. A market can use several lists,
              and shoppers get the lowest live price among them, so a markup list (+20%) can&apos;t
              win beside a market&apos;s regular list. You can change this on the list&apos;s page
              later.
            </Text>
          </VStack>
          {sameCurrency.length === 0 ? (
            <Text color="secondary">No market sells in {form.currency} yet.</Text>
          ) : (
            <MultiSelector
              label={`Markets in ${form.currency}`}
              placeholder="None for now"
              options={sameCurrency.map((m) => ({ value: m.id, label: m.name }))}
              value={chosenMarkets.filter((id) => sameCurrency.some((m) => m.id === id))}
              onChange={setChosenMarkets}
              width="100%"
            />
          )}
        </VStack>
      </Card>

      <HStack gap={2} hAlign="end">
        <Button label="Cancel" variant="ghost" href="/markets/price-lists" />
        <Button
          label="Create price list"
          variant="primary"
          isLoading={isPending}
          onClick={submit}
        />
      </HStack>

      {isPicking && (
        <ProductPicker
          title="Products the adjustment covers"
          isAlreadyIn={(p) => form.products.includes(p.id)}
          onConfirm={async (picked) => {
            setNames((before) => ({
              ...before,
              ...Object.fromEntries(picked.map((p) => [p.id, p.title])),
            }));
            set({ products: [...form.products, ...picked.map((p) => p.id)] });
            setIsPicking(false);
          }}
          onClose={() => setIsPicking(false)}
        />
      )}
    </VStack>
  );
}

/** A list's settings on its page, saved with the bar. */
export function EditPriceListForm({
  list,
  categories,
  groups,
  productNames,
  isReadOnly,
}: {
  list: PriceList;
  categories: { value: string; label: string }[];
  groups: { value: string; label: string }[];
  /** Titles of the products its adjustment names, those that could be read. */
  productNames: Record<string, string>;
  isReadOnly: boolean;
}) {
  const timeZone = useTimeZone();
  const [saved] = useState(() => formFromList(list, timeZone));
  const [form, setForm] = useState<ListForm>(saved);
  const [names, setNames] = useState(productNames);
  const [isPicking, setIsPicking] = useState(false);
  const [errors, setErrors] = useState<FormErrors>({});
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const isDirty = JSON.stringify(form) !== JSON.stringify(saved);

  const set = (change: Partial<ListForm>) => {
    setForm((before) => ({ ...before, ...change }));
    setErrors((before) => clearErrors(before, change));
  };
  const save = () => {
    const found = checkForm(form, timeZone);
    setErrors(found);
    if (Object.keys(found).length > 0) {
      setError("Some settings need fixing: see the marked fields.");
      return;
    }
    startTransition(async () => {
      const result = await updatePriceList(list.id, form);
      setErrors(result.fieldErrors ?? {});
      setError(
        result.error ??
          (result.fieldErrors ? "Some settings need fixing: see the marked fields." : undefined),
      );
    });
  };
  const discard = () => {
    setForm(saved);
    setErrors({});
    setError(undefined);
  };

  return (
    <VStack gap={5}>
      {!isReadOnly && (
        <SaveBar
          isDirty={isDirty}
          isPending={isPending}
          error={error}
          onSave={save}
          onDiscard={discard}
        />
      )}
      <SettingsFields
        form={form}
        errors={errors}
        set={set}
        isNew={false}
        isReadOnly={isReadOnly}
        categories={categories}
        groups={groups}
        productNames={names}
        onPickProducts={() => setIsPicking(true)}
      />
      {isPicking && (
        <ProductPicker
          title="Products the adjustment covers"
          isAlreadyIn={(p) => form.products.includes(p.id)}
          onConfirm={async (picked) => {
            setNames((before) => ({
              ...before,
              ...Object.fromEntries(picked.map((p) => [p.id, p.title])),
            }));
            set({ products: [...form.products, ...picked.map((p) => p.id)] });
            setIsPicking(false);
          }}
          onClose={() => setIsPicking(false)}
        />
      )}
    </VStack>
  );
}
