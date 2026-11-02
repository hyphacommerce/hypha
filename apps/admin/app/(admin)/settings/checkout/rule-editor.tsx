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
import { useState, useTransition } from "react";

import { loadAddressFormat } from "@/app/(admin)/address-actions";
import { AddressFields } from "@/components/address-fields";
import { useCurrencies } from "@/components/currencies";
import { ProductPicker } from "@/components/product-picker";
import {
  CONDITION_TYPES,
  type ConditionForm,
  FIELDS,
  FLAGS,
  PATHS,
  type RuleForm,
  type RuleOptions,
  type TestResult,
  VERDICTS,
  blankCondition,
  marketCurrencies,
  toBody,
} from "@/lib/checkout-rules";
import {
  ADDRESS_FIELDS,
  type AddressFormat,
  type AddressValues,
  type FieldProblems,
  asField,
} from "@/lib/address";

import { type RuleProblems, createRule, tryAddress, updateRule } from "./actions";

const MAX_CONDITIONS = 10;

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
  const router = useRouter();
  const currencies = useCurrencies();
  const [form, setForm] = useState(initial);
  const [problems, setProblems] = useState<RuleProblems>({});
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const [names, setNames] = useState(productNames);

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
              label="Name"
              description="For staff; shoppers see the message."
              value={form.name}
              onChange={(name) => set({ name })}
              isDisabled={!canWrite}
              status={at("name")}
            />
            <Switch
              label="Enabled"
              description="Off: carts don't show it, and the test below still tries it."
              value={form.enabled}
              onChange={(enabled) => set({ enabled })}
              isDisabled={!canWrite}
            />
          </VStack>
        </Card>

        <Card>
          <VStack gap={4}>
            <Text type="display-3" as="h2">
              Where it applies
            </Text>
            <SegmentedControl
              label="Shipping countries"
              value={form.countriesMode}
              onChange={(mode) => {
                if (mode === "all" || mode === "include" || mode === "exclude") {
                  set({ countriesMode: mode });
                }
              }}
            >
              <SegmentedControlItem value="all" label="Every country" />
              <SegmentedControlItem value="include" label="Only these" />
              <SegmentedControlItem value="exclude" label="All but these" />
            </SegmentedControl>
            {form.countriesMode !== "all" && (
              <MultiSelector
                label="Countries"
                options={options.countries}
                value={form.countries}
                onChange={(countries) => set({ countries })}
                hasSearch
                triggerDisplay="labels"
                status={at("applies_to.countries")}
              />
            )}
            <MultiSelector
              label="Markets"
              placeholder="Every market"
              options={options.markets}
              value={form.markets}
              onChange={(markets) => set({ markets })}
              triggerDisplay="labels"
              status={at("applies_to.markets")}
            />
            <MultiSelector
              label="Channels"
              placeholder="Every channel"
              options={options.channels}
              value={form.channels}
              onChange={(channels) => set({ channels })}
              triggerDisplay="labels"
              status={at("applies_to.channels")}
            />
            <MultiSelector
              label="Shipping options"
              description="Chosen: only while the shopper has one of them chosen, and the message shows under the options."
              placeholder="Any option"
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
                Conditions
              </Text>
              <Text type="supporting" color="secondary">
                All of them must hold. A condition on the address waits until the shopper enters
                one.
              </Text>
            </VStack>
            {problems.conditions && <Banner status="error" title={problems.conditions} />}
            {form.conditions.map((condition, i) => (
              <ConditionRow
                key={condition.key}
                index={i}
                condition={condition}
                options={options}
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
                  label="Add condition"
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
              What the shopper is told
            </Text>
            <SegmentedControl
              label="Outcome"
              value={form.severity}
              onChange={(severity) => {
                if (severity === "error" || severity === "warning") set({ severity });
              }}
            >
              <SegmentedControlItem value="error" label="Error: blocks checkout" />
              <SegmentedControlItem value="warning" label="Warning: only shown" />
            </SegmentedControl>
            <TextArea
              label="Message"
              description="Plain text, shown under the field. Say what to do."
              maxLength={300}
              rows={2}
              value={form.message}
              onChange={(message) => set({ message })}
              isDisabled={!canWrite}
              status={at("outcome.message")}
            />
            <Selector
              label="Shown under"
              options={PATHS}
              value={form.path}
              onChange={(path) => set({ path })}
              isDisabled={!canWrite}
              status={at("outcome.path")}
            />
            <TextInput
              label="Code"
              description="For storefronts and apps: snake_case, like po_box_not_allowed. Empty is rule."
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
              <Button label="Cancel" variant="ghost" href="/settings/checkout" />
              <Button
                label={id ? "Save rule" : "Create rule"}
                variant="primary"
                isLoading={isPending}
                isDisabled={form.name.trim() === "" || form.message.trim() === ""}
                onClick={save}
              />
            </HStack>
          </VStack>
        )}
      </VStack>

      <TryPanel id={id} form={form} options={options} initialFormat={tryFormat} />
    </Grid>
  );
}

function ConditionRow({
  index,
  condition: c,
  options,
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
  names: Record<string, string>;
  currencies: string[];
  canRemove: boolean;
  isDisabled: boolean;
  at: (path: string) => Status;
  onChange: (patch: Partial<ConditionForm>) => void;
  onRemove: () => void;
  onNamed: (names: Record<string, string>) => void;
}) {
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
              label={index === 0 ? "When" : "And"}
              options={CONDITION_TYPES}
              value={type}
              onChange={(value) => {
                const next = CONDITION_TYPES.find((t) => t.value === value);
                if (next) onChange({ type: next.value });
              }}
              isDisabled={isDisabled}
              status={at(path)}
            />
          </div>
          {canRemove && (
            <IconButton
              label={`Remove condition ${index + 1}`}
              icon={<TrashIcon />}
              variant="ghost"
              onClick={onRemove}
            />
          )}
        </HStack>
        {isField && (
          <Selector
            label="Field"
            options={FIELDS}
            value={c.field}
            onChange={(field) => onChange({ field })}
            isDisabled={isDisabled}
          />
        )}
        {(type === "field_matches" || type === "field_not_matches") && (
          <>
            <TextInput
              label="Pattern"
              description="A regular expression, like \bbox\b. It may match anywhere in the field."
              value={c.pattern}
              onChange={(pattern) => onChange({ pattern })}
              isDisabled={isDisabled}
              status={at(`${path}.pattern`)}
            />
            <Switch
              label="Ignore case"
              value={c.caseInsensitive}
              onChange={(caseInsensitive) => onChange({ caseInsensitive })}
              isDisabled={isDisabled}
            />
          </>
        )}
        {(type === "field_contains" || type === "field_not_contains") && (
          <TextArea
            label="Text"
            description="One per line; case doesn't matter."
            rows={3}
            value={c.values}
            onChange={(values) => onChange({ values })}
            isDisabled={isDisabled}
            status={at(`${path}.values`)}
          />
        )}
        {(type === "field_longer_than" || type === "field_shorter_than") && (
          <TextInput
            label="Characters"
            value={c.length}
            onChange={(length) => onChange({ length })}
            isDisabled={isDisabled}
            status={at(`${path}.length`)}
          />
        )}
        {isPostal && (
          <Grid columns={{ minWidth: 160, repeat: "fit" }} gap={3}>
            <TextArea
              label="Starting with"
              description="One per line: 35"
              rows={3}
              value={c.prefixes}
              onChange={(prefixes) => onChange({ prefixes })}
              isDisabled={isDisabled}
              status={at(`${path}.prefixes`)}
            />
            <TextArea
              label="Ranges"
              description="One per line: 35000-35999"
              rows={3}
              value={c.ranges}
              onChange={(ranges) => onChange({ ranges })}
              isDisabled={isDisabled}
              status={at(`${path}.ranges`)}
            />
            <TextArea
              label="Exactly"
              description="One per line"
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
            label="Region codes"
            description="One per line, as the address form stores them: CA, NY, M."
            rows={3}
            value={c.regions}
            onChange={(regions) => onChange({ regions })}
            isDisabled={isDisabled}
            status={at(`${path}.regions`)}
          />
        )}
        {(type === "address_is" || type === "address_is_not") && (
          <Selector
            label="What"
            options={FLAGS}
            value={c.flag}
            onChange={(flag) => onChange({ flag })}
            isDisabled={isDisabled}
          />
        )}
        {type === "verification_is" && (
          <MultiSelector
            label="Verdicts"
            description="Needs a verification app; without one, no verdict holds."
            options={VERDICTS}
            value={c.verdicts}
            onChange={(verdicts) => onChange({ verdicts })}
            triggerDisplay="labels"
            status={at(`${path}.verdicts`)}
          />
        )}
        {isCart && (
          <>
            <MultiSelector
              label="Categories"
              description="A product in a category below one counts too."
              placeholder="No category"
              options={options.categories}
              value={c.categories}
              onChange={(categories) => onChange({ categories })}
              hasSearch={options.categories.length > 12}
              triggerDisplay="labels"
              status={at(`${path}.categories`)}
            />
            <VStack gap={2}>
              <Text weight="medium">Products</Text>
              {c.products.length === 0 ? (
                <Text type="supporting" color="secondary">
                  None chosen.
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
                    label="Choose products"
                    variant="secondary"
                    size="sm"
                    onClick={() => setPicking(true)}
                  />
                  {c.products.length > 0 && (
                    <Button
                      label="Clear"
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
                title="Choose products"
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
                One amount per currency; a cart in a currency left empty doesn&apos;t match.
              </Text>
            )}
          </VStack>
        )}
        {isWeight && (
          <TextInput
            label="Grams"
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
}: {
  id?: string;
  form: RuleForm;
  options: RuleOptions;
  initialFormat: AddressFormat | null;
}) {
  const currencies = useCurrencies();
  const [country, setCountry] = useState(initialFormat?.country ?? "");
  // The country's form: its fields, labels, required fields and regions.
  const [format, setFormat] = useState(initialFormat);
  const [address, setAddress] = useState(EMPTY_ADDRESS);
  const [email, setEmail] = useState("");
  const [market, setMarket] = useState<string>();
  const [option, setOption] = useState<string>();
  const [result, setResult] = useState<TestResult>();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const [isLoadingFormat, startLoading] = useTransition();
  // A format problem from the last try, under its field.
  const problems: FieldProblems = Object.fromEntries(
    (result?.problems ?? []).flatMap((p) => {
      const field = asField(p.field);
      return field ? [[field, p.message]] : [];
    }),
  );

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
      });
      setError(outcome.error);
      setResult(outcome.result);
    });

  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Try an address
          </Text>
          <Text type="supporting" color="secondary">
            What a cart would show with this rule as it is in the form, beside your other enabled
            rules and the address&apos;s format. Nothing is saved.
          </Text>
        </VStack>
        <Selector
          label="Country"
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
        <TextInput label="Email" isOptional value={email} onChange={setEmail} />
        {options.markets.length > 0 && (
          <Selector
            label="Market"
            isOptional
            options={options.markets}
            value={market}
            onChange={setMarket}
          />
        )}
        {options.shippingOptions.length > 0 && (
          <Selector
            label="Chosen shipping option"
            isOptional
            options={options.shippingOptions}
            value={option}
            onChange={setOption}
          />
        )}
        <HStack>
          <Button label="Try it" variant="secondary" isLoading={isPending} onClick={run} />
        </HStack>
        {error && <Banner status="error" title={error} />}
        {result && <TryResult result={result} />}
      </VStack>
    </Card>
  );
}

function TryResult({ result }: { result: TestResult }) {
  if (result.problems.length > 0) {
    return (
      <Banner
        status="error"
        title="A cart would refuse this address"
        description={result.problems.map((p) => p.message).join(" ")}
      />
    );
  }
  const flags = Object.entries(result.flags ?? {})
    .filter(([, on]) => on)
    .map(([flag]) => FLAGS.find((f) => f.value === flag)?.label ?? flag);
  return (
    <VStack gap={3}>
      {flags.length > 0 && (
        <Text type="supporting" color="secondary">
          The address reads as {flags.join(", ")}.
        </Text>
      )}
      {result.messages.length === 0 ? (
        <Banner status="success" title="No messages: checkout would go on." />
      ) : (
        result.messages.map((m) => (
          <Banner
            // A rule says one thing, and the format one thing per field.
            key={`${m.source.type}:${"rule" in m.source ? m.source.rule : ""}:${m.code}:${m.path}`}
            status={m.severity === "error" ? "error" : "warning"}
            title={m.message}
            description={`${m.severity === "error" ? "Blocks checkout" : "Warning"} · ${m.path}${m.draft ? " · this rule" : ""}`}
          />
        ))
      )}
    </VStack>
  );
}
