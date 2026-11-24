"use client";

// A checkout rule's page: where it applies, its conditions, what the shopper is told,
// and a "Try an address" panel that asks the API what a cart would show with the rule
// as it stands in the form, saved or not.

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Grid } from "@astryxdesign/core/Grid";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { SegmentedControl, SegmentedControlItem } from "@astryxdesign/core/SegmentedControl";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import { TrashIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useMemo, useState, useTransition } from "react";

import { loadAddressFormat } from "@/app/(admin)/address-actions";
import { AddressFields } from "@/components/address-fields";
import { useCurrencies } from "@/components/currencies";
import { ProductPicker } from "@/components/product-picker";
import {
  ADDRESS_FIELDS,
  type AddressFormat,
  type AddressValues,
  problemsByField,
  wordAddressMessage,
} from "@/lib/address";
import {
  CONDITION_TYPES,
  type ConditionForm,
  type RuleForm,
  type RuleOptions,
  type TestResult,
  blankCondition,
  conditionTypeOptions,
  fieldOptions,
  flagLabel,
  flagOptions,
  marketCurrencies,
  pathOptions,
  toBody,
  verdictOptions,
  verdictPhrase,
} from "@/lib/checkout-rules";
import type { TFor } from "@/lib/messages";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { type RuleProblems, createRule, tryAddress, updateRule } from "./actions";

const MAX_CONDITIONS = 10;

/** The pickers' choices in the staff member's language, built once per editor. */
function choicesOf(t: TFor<"checkout">) {
  return {
    kinds: conditionTypeOptions(t),
    fields: fieldOptions(t),
    flags: flagOptions(t),
    verdicts: verdictOptions(t),
    paths: pathOptions(t),
  };
}

type Choices = ReturnType<typeof choicesOf>;

type Status = { type: "error"; message: string } | undefined;

export function RuleEditor({
  id,
  initial,
  options,
  productNames,
  canWrite,
  tryFormat,
}: {
  /** The saved rule's ID; none for a new one. */
  id?: string;
  initial: RuleForm;
  options: RuleOptions;
  /** The address form "Try an address" starts with. */
  tryFormat: AddressFormat | null;
  /** Titles of the products the rule names, by ID. */
  productNames: Record<string, string>;
  canWrite: boolean;
}) {
  const t = useT<"checkout">();
  const router = useRouter();
  const currencies = useCurrencies();
  const [form, setForm] = useState(initial);
  const [problems, setProblems] = useState<RuleProblems>({});
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const [names, setNames] = useState(productNames);
  const choices = useMemo(() => choicesOf(t), [t]);

  const set = (patch: Partial<RuleForm>) => setForm((f) => ({ ...f, ...patch }));
  const setCondition = (i: number, patch: Partial<ConditionForm>) =>
    setForm((f) => ({
      ...f,
      conditions: f.conditions.map((c, j) => (j === i ? { ...c, ...patch } : c)),
    }));

  /** The first problem at `path` or inside it. */
  const at = (path: string): Status => {
    const key = Object.keys(problems).find(
      (k) => k === path || k.startsWith(`${path}.`) || k.startsWith(`${path}[`),
    );
    return key ? { type: "error", message: problems[key] } : undefined;
  };

  const save = () =>
    startTransition(async () => {
      const body = toBody(form, currencies);
      const result = id ? await updateRule(id, body) : await createRule(body);
      setProblems(result.problems ?? {});
      setError(result.error);
      if (!result.error && result.id) {
        if (id) router.refresh();
        else router.push(`/settings/checkout/rules/${result.id}`);
      }
    });

  return (
    <Grid columns={{ minWidth: 360, repeat: "fit" }} gap={5}>
      <VStack gap={5}>
        <Card>
          <VStack gap={4}>
            <TextInput
              label={t("common.name")}
              description={t("checkout.editor_name_description")}
              value={form.name}
              onChange={(name) => set({ name })}
              isDisabled={!canWrite}
              status={at("name")}
            />
            <Switch
              label={t("common.enabled")}
              description={t("checkout.editor_enabled_description")}
              value={form.enabled}
              onChange={(enabled) => set({ enabled })}
              isDisabled={!canWrite}
            />
          </VStack>
        </Card>

        <Card>
          <VStack gap={4}>
            <Text type="display-3" as="h2">
              {t("checkout.editor_applies_title")}
            </Text>
            <SegmentedControl
              label={t("checkout.editor_countries_mode")}
              value={form.countriesMode}
              onChange={(mode) => {
                if (mode === "all" || mode === "include" || mode === "exclude") {
                  set({ countriesMode: mode });
                }
              }}
            >
              <SegmentedControlItem value="all" label={t("checkout.editor_countries_all")} />
              <SegmentedControlItem
                value="include"
                label={t("checkout.editor_countries_include")}
              />
              <SegmentedControlItem
                value="exclude"
                label={t("checkout.editor_countries_exclude")}
              />
            </SegmentedControl>
            {form.countriesMode !== "all" && (
              <MultiSelector
                label={t("common.countries")}
                options={options.countries}
                value={form.countries}
                onChange={(countries) => set({ countries })}
                hasSearch
                triggerDisplay="labels"
                status={at("applies_to.countries")}
              />
            )}
            <MultiSelector
              label={t("common.markets")}
              placeholder={t("common.every_market")}
              options={options.markets}
              value={form.markets}
              onChange={(markets) => set({ markets })}
              triggerDisplay="labels"
              status={at("applies_to.markets")}
            />
            <MultiSelector
              label={t("common.channels")}
              placeholder={t("common.every_channel")}
              options={options.channels}
              value={form.channels}
              onChange={(channels) => set({ channels })}
              triggerDisplay="labels"
              status={at("applies_to.channels")}
            />
            <MultiSelector
              label={t("checkout.editor_shipping_options")}
              description={t("checkout.editor_shipping_options_description")}
              placeholder={t("checkout.editor_any_option")}
              options={options.shippingOptions}
              value={form.shippingOptions}
              onChange={(shippingOptions) => set({ shippingOptions })}
              hasSearch={options.shippingOptions.length > 12}
              triggerDisplay="labels"
              status={at("applies_to.shipping_options")}
            />
          </VStack>
        </Card>

        <Card>
          <VStack gap={4}>
            <VStack gap={1}>
              <Text type="display-3" as="h2">
                {t("checkout.editor_conditions_title")}
              </Text>
              <Text type="supporting" color="secondary">
                {t("checkout.editor_conditions_description")}
              </Text>
            </VStack>
            {problems.conditions && <Banner status="error" title={problems.conditions} />}
            {form.conditions.map((condition, i) => (
              <ConditionRow
                key={condition.key}
                index={i}
                condition={condition}
                options={options}
                choices={choices}
                names={names}
                currencies={marketCurrencies(options)}
                canRemove={form.conditions.length > 1 && canWrite}
                isDisabled={!canWrite}
                at={at}
                onChange={(patch) => setCondition(i, patch)}
                onRemove={() => set({ conditions: form.conditions.filter((_, j) => j !== i) })}
                onNamed={(more) => setNames((n) => ({ ...n, ...more }))}
              />
            ))}
            {canWrite && (
              <HStack>
                <Button
                  label={t("checkout.editor_condition_add")}
                  variant="secondary"
                  isDisabled={form.conditions.length >= MAX_CONDITIONS}
                  onClick={() => set({ conditions: [...form.conditions, blankCondition()] })}
                />
              </HStack>
            )}
          </VStack>
        </Card>

        <Card>
          <VStack gap={4}>
            <Text type="display-3" as="h2">
              {t("checkout.editor_outcome_title")}
            </Text>
            <SegmentedControl
              label={t("checkout.editor_outcome")}
              value={form.severity}
              onChange={(severity) => {
                if (severity === "error" || severity === "warning") set({ severity });
              }}
            >
              <SegmentedControlItem value="error" label={t("checkout.editor_outcome_error")} />
              <SegmentedControlItem value="warning" label={t("checkout.editor_outcome_warning")} />
            </SegmentedControl>
            <TextArea
              label={t("checkout.editor_message")}
              description={t("checkout.editor_message_description")}
              maxLength={300}
              rows={2}
              value={form.message}
              onChange={(message) => set({ message })}
              isDisabled={!canWrite}
              status={at("outcome.message")}
            />
            <Selector
              label={t("checkout.editor_path")}
              options={choices.paths}
              value={form.path}
              onChange={(path) => set({ path })}
              isDisabled={!canWrite}
              status={at("outcome.path")}
            />
            <TextInput
              label={t("common.code")}
              description={t("checkout.editor_code_description")}
              isOptional
              value={form.code}
              onChange={(code) => set({ code })}
              isDisabled={!canWrite}
              status={at("outcome.code")}
            />
          </VStack>
        </Card>

        {canWrite && (
          <VStack gap={3}>
            {error && <Banner status="error" title={error} />}
            <HStack gap={2} hAlign="end">
              <Button label={t("common.cancel")} variant="ghost" href="/settings/checkout" />
              <Button
                label={id ? t("checkout.editor_save") : t("checkout.editor_create")}
                variant="primary"
                isLoading={isPending}
                isDisabled={form.name.trim() === "" || form.message.trim() === ""}
                onClick={save}
              />
            </HStack>
          </VStack>
        )}
      </VStack>

      <TryPanel
        id={id}
        form={form}
        options={options}
        initialFormat={tryFormat}
        canVerify={canWrite}
      />
    </Grid>
  );
}

function ConditionRow({
  index,
  condition: c,
  options,
  choices,
  names,
  currencies,
  canRemove,
  isDisabled,
  at,
  onChange,
  onRemove,
  onNamed,
}: {
  index: number;
  condition: ConditionForm;
  options: RuleOptions;
  choices: Choices;
  names: Record<string, string>;
  currencies: string[];
  canRemove: boolean;
  isDisabled: boolean;
  at: (path: string) => Status;
  onChange: (patch: Partial<ConditionForm>) => void;
  onRemove: () => void;
  onNamed: (names: Record<string, string>) => void;
}) {
  const t = useT<"checkout">();
  const [isPicking, setPicking] = useState(false);
  const path = `conditions[${index}]`;
  const type = c.type;
  const isField = type.startsWith("field_");
  const isPostal = type === "postal_code_in" || type === "postal_code_not_in";
  const isCart = type === "cart_contains" || type === "cart_not_contains";
  const isSubtotal = type === "subtotal_at_least" || type === "subtotal_below";
  const isWeight = type === "weight_at_least" || type === "weight_below";
  return (
    <Card variant="muted" padding={3}>
      <VStack gap={3}>
        <HStack gap={2} vAlign="end">
          <div style={{ flex: "1 1 auto" }}>
            <Selector
              label={
                index === 0
                  ? t("checkout.editor_condition_when")
                  : t("checkout.editor_condition_and")
              }
              options={choices.kinds}
              value={type}
              onChange={(value) => {
                const next = CONDITION_TYPES.find((kind) => kind.value === value);
                if (next) onChange({ type: next.value });
              }}
              isDisabled={isDisabled}
              status={at(path)}
            />
          </div>
          {canRemove && (
            <IconButton
              label={t("checkout.editor_condition_remove", { number: index + 1 })}
              icon={<TrashIcon />}
              variant="ghost"
              onClick={onRemove}
            />
          )}
        </HStack>
        {isField && (
          <Selector
            label={t("checkout.editor_field")}
            options={choices.fields}
            value={c.field}
            onChange={(field) => onChange({ field })}
            isDisabled={isDisabled}
          />
        )}
        {(type === "field_matches" || type === "field_not_matches") && (
          <>
            <TextInput
              label={t("checkout.editor_pattern")}
              description={t("checkout.editor_pattern_description")}
              value={c.pattern}
              onChange={(pattern) => onChange({ pattern })}
              isDisabled={isDisabled}
              status={at(`${path}.pattern`)}
            />
            <Switch
              label={t("checkout.editor_ignore_case")}
              value={c.caseInsensitive}
              onChange={(caseInsensitive) => onChange({ caseInsensitive })}
              isDisabled={isDisabled}
            />
          </>
        )}
        {(type === "field_contains" || type === "field_not_contains") && (
          <TextArea
            label={t("checkout.editor_text")}
            description={t("checkout.editor_text_description")}
            rows={3}
            value={c.values}
            onChange={(values) => onChange({ values })}
            isDisabled={isDisabled}
            status={at(`${path}.values`)}
          />
        )}
        {(type === "field_longer_than" || type === "field_shorter_than") && (
          <TextInput
            label={t("checkout.editor_characters")}
            value={c.length}
            onChange={(length) => onChange({ length })}
            isDisabled={isDisabled}
            status={at(`${path}.length`)}
          />
        )}
        {isPostal && (
          <Grid columns={{ minWidth: 160, repeat: "fit" }} gap={3}>
            <TextArea
              label={t("checkout.editor_postal_prefixes")}
              description={t("checkout.editor_postal_prefixes_description")}
              rows={3}
              value={c.prefixes}
              onChange={(prefixes) => onChange({ prefixes })}
              isDisabled={isDisabled}
              status={at(`${path}.prefixes`)}
            />
            <TextArea
              label={t("checkout.editor_postal_ranges")}
              description={t("checkout.editor_postal_ranges_description")}
              rows={3}
              value={c.ranges}
              onChange={(ranges) => onChange({ ranges })}
              isDisabled={isDisabled}
              status={at(`${path}.ranges`)}
            />
            <TextArea
              label={t("checkout.editor_postal_codes")}
              description={t("checkout.editor_postal_codes_description")}
              rows={3}
              value={c.codes}
              onChange={(codes) => onChange({ codes })}
              isDisabled={isDisabled}
              status={at(`${path}.codes`)}
            />
          </Grid>
        )}
        {(type === "region_in" || type === "region_not_in") && (
          <TextArea
            label={t("checkout.editor_region_codes")}
            description={t("checkout.editor_region_codes_description")}
            rows={3}
            value={c.regions}
            onChange={(regions) => onChange({ regions })}
            isDisabled={isDisabled}
            status={at(`${path}.regions`)}
          />
        )}
        {(type === "address_is" || type === "address_is_not") && (
          <Selector
            label={t("checkout.editor_flag")}
            options={choices.flags}
            value={c.flag}
            onChange={(flag) => onChange({ flag })}
            isDisabled={isDisabled}
          />
        )}
        {type === "verification_is" && (
          <MultiSelector
            label={t("checkout.editor_verdicts")}
            description={t("checkout.editor_verdicts_description")}
            options={choices.verdicts}
            value={c.verdicts}
            onChange={(verdicts) => onChange({ verdicts })}
            triggerDisplay="labels"
            status={at(`${path}.verdicts`)}
          />
        )}
        {isCart && (
          <>
            <MultiSelector
              label={t("common.categories")}
              description={t("checkout.editor_categories_description")}
              placeholder={t("checkout.editor_no_category")}
              options={options.categories}
              value={c.categories}
              onChange={(categories) => onChange({ categories })}
              hasSearch={options.categories.length > 12}
              triggerDisplay="labels"
              status={at(`${path}.categories`)}
            />
            <VStack gap={2}>
              <Text weight="medium">{t("common.products")}</Text>
              {c.products.length === 0 ? (
                <Text type="supporting" color="secondary">
                  {t("checkout.editor_products_none")}
                </Text>
              ) : (
                <HStack gap={1} wrap="wrap">
                  {c.products.map((p) => (
                    <Badge key={p} label={names[p] ?? p} />
                  ))}
                </HStack>
              )}
              {!isDisabled && (
                <HStack gap={2}>
                  <Button
                    label={t("checkout.editor_products_choose")}
                    variant="secondary"
                    size="sm"
                    onClick={() => setPicking(true)}
                  />
                  {c.products.length > 0 && (
                    <Button
                      label={t("common.clear")}
                      variant="ghost"
                      size="sm"
                      onClick={() => onChange({ products: [] })}
                    />
                  )}
                </HStack>
              )}
            </VStack>
            {isPicking && (
              <ProductPicker
                title={t("checkout.editor_products_picker_title")}
                isAlreadyIn={(p) => c.products.includes(p.id)}
                onConfirm={async (picked) => {
                  onNamed(Object.fromEntries(picked.map((p) => [p.id, p.title])));
                  onChange({ products: [...c.products, ...picked.map((p) => p.id)] });
                  setPicking(false);
                }}
                onClose={() => setPicking(false)}
              />
            )}
          </>
        )}
        {isSubtotal && (
          <VStack gap={2}>
            <Grid columns={{ minWidth: 140, repeat: "fit" }} gap={3}>
              {currencies.map((currency) => (
                <TextInput
                  key={currency}
                  label={currency}
                  isOptional
                  value={c.amounts[currency] ?? ""}
                  onChange={(amount) => onChange({ amounts: { ...c.amounts, [currency]: amount } })}
                  isDisabled={isDisabled}
                />
              ))}
            </Grid>
            {at(`${path}.amounts`) ? (
              <Banner status="error" title={at(`${path}.amounts`)?.message ?? ""} />
            ) : (
              <Text type="supporting" color="secondary">
                {t("checkout.editor_amounts_hint")}
              </Text>
            )}
          </VStack>
        )}
        {isWeight && (
          <TextInput
            label={t("checkout.editor_grams")}
            value={c.grams}
            onChange={(grams) => onChange({ grams })}
            isDisabled={isDisabled}
          />
        )}
      </VStack>
    </Card>
  );
}

// Names never decide a rule, so the panel doesn't ask for them: the last name is there
// for the format's required field.
const EMPTY_ADDRESS: AddressValues = {
  first_name: "",
  last_name: "Test",
  company: "",
  line1: "",
  line2: "",
  city: "",
  province: "",
  postal_code: "",
  phone: "",
};

/** The fields the panel asks for. */
const TRIED = new Set<string>(
  ADDRESS_FIELDS.filter((f) => f !== "first_name" && f !== "last_name" && f !== "company"),
);

/** `format` without the names, and with the phone (a rule may ask for one) where it has none. */
function tryLayout(format: AddressFormat): AddressFormat {
  const rows = format.rows.map((row) => row.filter((f) => TRIED.has(f))).filter((r) => r.length);
  if (!rows.some((row) => row.includes("phone"))) rows.push(["phone"]);
  return { ...format, rows };
}

/** An empty input as `null`, as the API takes an absent field. */
function blank(value: string): string | null {
  return value.trim() === "" ? null : value;
}

/** "Try an address": what a cart would show for it, with the rule as it stands. */
function TryPanel({
  id,
  form,
  options,
  initialFormat,
  canVerify,
}: {
  id?: string;
  form: RuleForm;
  options: RuleOptions;
  initialFormat: AddressFormat | null;
  /** Whether the caller may ask the verification app (`checkout_rules:write`: it spends a
   * lookup). */
  canVerify: boolean;
}) {
  const t = useT<"checkout">();
  const currencies = useCurrencies();
  const [country, setCountry] = useState(initialFormat?.country ?? "");
  // The country's form: its fields, labels, required fields and regions.
  const [format, setFormat] = useState(initialFormat);
  const [address, setAddress] = useState(EMPTY_ADDRESS);
  const [email, setEmail] = useState("");
  const [market, setMarket] = useState<string>();
  const [option, setOption] = useState<string>();
  const [verify, setVerify] = useState(false);
  const [result, setResult] = useState<TestResult>();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const [isLoadingFormat, startLoading] = useTransition();
  // A format problem from the last try, under its field.
  const problems = problemsByField(result?.problems ?? []);

  const chooseCountry = (next: string) => {
    setCountry(next);
    // Another country's region and postal code mean nothing here.
    setAddress((a) => ({ ...a, province: "", postal_code: "" }));
    setResult(undefined);
    startLoading(async () => setFormat(await loadAddressFormat(next)));
  };

  const run = () =>
    startTransition(async () => {
      const outcome = await tryAddress({
        address: {
          last_name: address.last_name,
          line1: address.line1,
          line2: blank(address.line2),
          city: address.city,
          province: blank(address.province),
          postal_code: blank(address.postal_code),
          phone: blank(address.phone),
          country,
        },
        email: blank(email),
        market: market ?? null,
        shipping_option: option ?? null,
        rule: toBody(form, currencies),
        rule_id: id ?? null,
        verify,
      });
      setError(outcome.error);
      setResult(outcome.result);
    });

  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("checkout.try_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("checkout.try_description")}
          </Text>
        </VStack>
        <Selector
          label={t("common.country")}
          options={options.countries}
          value={country || undefined}
          onChange={chooseCountry}
          hasSearch
        />
        {format && (
          <div style={{ opacity: isLoadingFormat ? 0.6 : 1 }}>
            <AddressFields
              format={tryLayout(format)}
              values={address}
              onChange={(field, value) => setAddress((a) => ({ ...a, [field]: value }))}
              problems={problems}
              tried={false}
              onFill={(values) => setAddress((a) => ({ ...a, ...values }))}
            />
          </div>
        )}
        <TextInput label={t("common.email")} isOptional value={email} onChange={setEmail} />
        {options.markets.length > 0 && (
          <Selector
            label={t("common.market")}
            isOptional
            options={options.markets}
            value={market}
            onChange={setMarket}
          />
        )}
        {options.shippingOptions.length > 0 && (
          <Selector
            label={t("checkout.try_shipping_option")}
            isOptional
            options={options.shippingOptions}
            value={option}
            onChange={setOption}
          />
        )}
        {canVerify && (
          <Switch
            label={t("checkout.try_verify")}
            description={t("checkout.try_verify_description")}
            value={verify}
            onChange={setVerify}
          />
        )}
        <HStack>
          <Button
            label={t("checkout.try_run")}
            variant="secondary"
            isLoading={isPending}
            onClick={run}
          />
        </HStack>
        {error && <Banner status="error" title={error} />}
        {result && <TryResult result={result} format={format} />}
      </VStack>
    </Card>
  );
}

function TryResult({
  result,
  format: addressFormat,
}: {
  result: TestResult;
  /** The address's country's form, for its fields' labels. */
  format: AddressFormat | null;
}) {
  const t = useT<"checkout">();
  const format = useFormat();
  if (result.problems.length > 0) {
    return (
      <Banner
        status="error"
        title={t("checkout.try_refused_title")}
        description={
          // One problem a line: each is a sentence of its own.
          <VStack gap={1}>
            {result.problems.map((p) => (
              <span key={`${p.field}:${p.code}`}>{wordAddressMessage(t, p, addressFormat)}</span>
            ))}
          </VStack>
        }
      />
    );
  }
  const flags = Object.entries(result.flags ?? {})
    .filter(([, on]) => on)
    .map(([flag]) => flagLabel(t, flag));
  const verification = result.verification;
  return (
    <VStack gap={3}>
      {verification && (
        <Text type="supporting" color="secondary">
          {verification.failure
            ? t("checkout.try_verification_failed", {
                verdict: verdictPhrase(t, verification.verdict),
                failure: verification.failure,
              })
            : t("checkout.try_verification", {
                verdict: verdictPhrase(t, verification.verdict),
              })}
        </Text>
      )}
      {flags.length > 0 && (
        <Text type="supporting" color="secondary">
          {t("checkout.try_flags", { flags: format.list(flags, "unit") })}
        </Text>
      )}
      {result.messages.length === 0 ? (
        <Banner status="success" title={t("checkout.try_no_messages")} />
      ) : (
        result.messages.map((m) => (
          <Banner
            // A rule says one thing, and the format one thing per field.
            key={`${m.source.type}:${"rule" in m.source ? m.source.rule : ""}:${m.code}:${m.path}`}
            status={m.severity === "error" ? "error" : "warning"}
            title={wordAddressMessage(t, m, addressFormat)}
            description={[
              m.severity === "error" ? t("checkout.try_blocks") : t("checkout.try_warning"),
              m.path,
              ...(m.draft ? [t("checkout.try_this_rule")] : []),
            ].join(" · ")}
          />
        ))
      )}
    </VStack>
  );
}
