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
import { parsePercent } from "@/lib/price-grid/money";
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
  percentLabel,
  toLocalInput,
} from "@/lib/price-lists";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

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
  const t = useT<"price_lists">();
  const format = useFormat();
  const currencies = useCurrencies();
  const timeZone = useTimeZone();
  const typed = parsePercent(form.percent);
  const bps = typed.ok ? typed.value : null;
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
            {t("common.details")}
          </Text>
          <TextInput
            label={t("common.name")}
            description={t("price_lists.form_name_description")}
            placeholder={t("price_lists.form_name_placeholder")}
            value={form.name}
            onChange={(name) => set({ name })}
            isDisabled={isReadOnly}
            status={status(errors.name)}
            width="100%"
          />
          <TextArea
            label={t("common.description")}
            isOptional
            rows={2}
            maxLength={255}
            value={form.description}
            onChange={(description) => set({ description })}
            isDisabled={isReadOnly}
            status={status(errors.description)}
          />
          <Selector
            label={t("common.currency")}
            description={
              isNew
                ? t("price_lists.form_currency_description_new")
                : t("price_lists.form_currency_description_fixed")
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
            label={t("price_lists.form_kind")}
            value={form.kind}
            onChange={(kind) => set({ kind: kind === "sale" ? "sale" : "override" })}
            isDisabled={isReadOnly}
          >
            {KINDS.map((kind) => (
              <RadioListItem
                key={kind}
                value={kind}
                label={t(`price_lists.kind_${kind}`)}
                description={t(`price_lists.kind_${kind}_description`)}
              />
            ))}
          </RadioList>
          {form.kind === "sale" && (
            <Text type="supporting" color="secondary">
              {t("price_lists.form_eu_notice")}
            </Text>
          )}
          {form.kind === "override" && (
            <RadioList
              label={t("price_lists.form_compare_at")}
              value={form.compareAt}
              onChange={(compareAt) =>
                set({ compareAt: compareAt === "hidden" ? "hidden" : "adjusted" })
              }
              isDisabled={isReadOnly}
            >
              <RadioListItem
                value="adjusted"
                label={t("price_lists.form_compare_at_adjusted")}
                description={t("price_lists.form_compare_at_adjusted_description")}
              />
              <RadioListItem
                value="hidden"
                label={t("price_lists.form_compare_at_hidden")}
                description={t("price_lists.form_compare_at_hidden_description")}
              />
            </RadioList>
          )}
        </VStack>
      </Card>

      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("price_lists.form_schedule")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("price_lists.form_schedule_description", { timeZone })}
            </Text>
          </VStack>
          <Switch
            label={t("price_lists.form_active")}
            description={t("price_lists.form_active_description")}
            value={form.status === "active"}
            onChange={(on) => set({ status: on ? "active" : "draft" })}
            isDisabled={isReadOnly}
          />
          <HStack gap={4} wrap="wrap">
            <DateTimeInput
              label={t("price_lists.form_starts")}
              description={t("price_lists.form_starts_description")}
              value={isMinute(form.startsAt) ? form.startsAt : undefined}
              onChange={(value) => set({ startsAt: value ?? "" })}
              hasClear
              hourFormat="24h"
              weekStartsOn="mon"
              isDisabled={isReadOnly}
              status={status(errors.startsAt)}
            />
            <DateTimeInput
              label={t("price_lists.form_ends")}
              description={t("price_lists.form_ends_description")}
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
              {t("price_lists.form_audience")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("price_lists.form_audience_description")}
            </Text>
          </VStack>
          {groupOptions.length === 0 ? (
            <Text color="secondary">
              {t.rich("price_lists.form_audience_no_groups", {
                link: (chunks, key) => (
                  <Link key={key} href="/customers/groups">
                    {chunks}
                  </Link>
                ),
              })}
            </Text>
          ) : (
            <MultiSelector
              label={t("price_lists.form_groups")}
              placeholder={t("price_lists.form_groups_placeholder")}
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
              {t("price_lists.form_adjustment")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("price_lists.form_adjustment_description")}
            </Text>
          </VStack>
          <Switch
            label={t("price_lists.form_adjust_base_prices")}
            value={form.hasAdjustment}
            onChange={(hasAdjustment) =>
              set({ hasAdjustment, percent: hasAdjustment && !form.percent ? "-10" : form.percent })
            }
            isDisabled={isReadOnly}
          />
          {form.hasAdjustment && (
            <>
              <TextInput
                label={t("price_lists.form_percentage")}
                description={
                  bps === null
                    ? t("price_lists.form_percentage_hint")
                    : bps === 0
                      ? t("price_lists.form_percentage_zero")
                      : t(
                          bps < 0
                            ? "price_lists.form_percentage_reduction"
                            : "price_lists.form_percentage_markup",
                          { percent: percentLabel(format, bps) },
                        )
                }
                value={form.percent}
                onChange={(percent) => set({ percent })}
                isDisabled={isReadOnly}
                status={status(errors.percent)}
                width={200}
              />
              <RadioList
                label={t("price_lists.form_applies_to")}
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
                <RadioListItem value="all" label={t("price_lists.form_scope_all")} />
                <RadioListItem
                  value="categories"
                  label={t("price_lists.form_scope_categories")}
                  description={t("price_lists.form_scope_categories_description")}
                />
                <RadioListItem value="products" label={t("price_lists.form_scope_products")} />
              </RadioList>
              {form.scope === "categories" && (
                <VStack gap={2}>
                  <MultiSelector
                    label={t("common.categories")}
                    placeholder={t("price_lists.form_categories_placeholder")}
                    options={categories}
                    value={form.categories}
                    onChange={(chosen) => set({ categories: chosen })}
                    hasSearch
                    triggerDisplay="count"
                    formatValue={(items) =>
                      t("price_lists.form_categories_count", { count: items.length })
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
                    <Button
                      label={t("price_lists.form_choose_products")}
                      variant="secondary"
                      onClick={onPickProducts}
                    />
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
  const t = useT<"price_lists">();
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
    const found = TEMPLATES.find((candidate) => candidate.key === key);
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
    const found = checkForm(t, form, timeZone);
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
              {t("price_lists.form_start_from")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("price_lists.form_start_from_description")}
            </Text>
          </VStack>
          <div className="template-grid">
            {TEMPLATES.map(({ key }) => (
              <SelectableCard
                key={key}
                label={t(`price_lists.template_${key}`)}
                isSelected={template === key}
                onChange={(isSelected) => isSelected && choose(key)}
                padding={3}
              >
                <VStack gap={1}>
                  <Text weight="semibold">{t(`price_lists.template_${key}`)}</Text>
                  <Text type="supporting" color="secondary">
                    {t(`price_lists.template_${key}_description`)}
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
              {t("common.markets")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("price_lists.form_markets_description")}
            </Text>
          </VStack>
          {sameCurrency.length === 0 ? (
            <Text color="secondary">
              {t("price_lists.form_no_market_in_currency", { currency: form.currency })}
            </Text>
          ) : (
            <MultiSelector
              label={t("price_lists.form_markets_in_currency", { currency: form.currency })}
              placeholder={t("price_lists.form_markets_placeholder")}
              options={sameCurrency.map((m) => ({ value: m.id, label: m.name }))}
              value={chosenMarkets.filter((id) => sameCurrency.some((m) => m.id === id))}
              onChange={setChosenMarkets}
              width="100%"
            />
          )}
        </VStack>
      </Card>

      <HStack gap={2} hAlign="end">
        <Button label={t("common.cancel")} variant="ghost" href="/markets/price-lists" />
        <Button
          label={t("price_lists.form_create")}
          variant="primary"
          isLoading={isPending}
          onClick={submit}
        />
      </HStack>

      {isPicking && (
        <ProductPicker
          title={t("price_lists.form_products_picker_title")}
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
  const t = useT<"price_lists">();
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
    const found = checkForm(t, form, timeZone);
    setErrors(found);
    if (Object.keys(found).length > 0) {
      setError(t("price_lists.form_needs_fixing"));
      return;
    }
    startTransition(async () => {
      const result = await updatePriceList(list.id, form);
      setErrors(result.fieldErrors ?? {});
      setError(
        result.error ?? (result.fieldErrors ? t("price_lists.form_needs_fixing") : undefined),
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
          title={t("price_lists.form_products_picker_title")}
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
