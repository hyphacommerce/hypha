"use client";

// Checkout's forms (design note 0024, §7): contact and address, a shipping option per
// group, how the shopper pays, and "Place order" (a manual method) or the payment
// itself (an online one, paid before the order exists: design note 0031). Each saves
// through its server action, which answers with the page re-rendered from the cart
// (`refresh()`).

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Grid } from "@astryxdesign/core/Grid";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type ElementEvent, HyphaElement } from "@hypha/storefront-elements/react";
import { useState, useTransition } from "react";

import {
  type CartPaymentResult,
  type Details,
  placeOrder,
  saveDetails,
  selectPaymentMethod,
  selectShipping,
  startCartPayment,
} from "@/app/[country]/actions";

/** The fields the form won't save without (marked required below). */
const REQUIRED: (keyof Details)[] = ["email", "last_name", "line1", "city"];

/** Contact and shipping address, saved together. */
export function DetailsForm({
  country,
  countryName,
  initial,
  saved,
}: {
  country: string;
  countryName: string;
  initial: Details;
  /** Whether the cart already has them (the button says "Update"). */
  saved: boolean;
}) {
  const [details, setDetails] = useState(initial);
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();
  const [tried, setTried] = useState(false);

  const set = (key: keyof Details) => (value: string) =>
    setDetails((d) => ({ ...d, [key]: value }));
  const required = (key: keyof Details) =>
    tried && details[key].trim() === ""
      ? { type: "error" as const, message: "Required" }
      : undefined;

  const submit = () => {
    setTried(true);
    if (REQUIRED.some((k) => details[k].trim() === "")) return;
    start(async () => {
      // A save answers with the page re-rendered (`refresh` in the action).
      const result = await saveDetails(country, details);
      setError(result?.error);
    });
  };

  const field = (
    key: keyof Details,
    label: string,
    options: { required?: boolean; type?: "email" } = {},
  ) => (
    <TextInput
      size="lg"
      label={label}
      type={options.type}
      isRequired={options.required}
      isOptional={!options.required}
      value={details[key]}
      onChange={set(key)}
      status={options.required ? required(key) : undefined}
    />
  );

  return (
    <VStack gap={6}>
      <VStack gap={3}>
        <Text type="large" weight="bold">
          Contact
        </Text>
        {field("email", "Email", { required: true, type: "email" })}
      </VStack>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="large" weight="bold">
            Shipping address
          </Text>
          <Text type="supporting">
            Ships to {countryName}. To ship elsewhere, switch the currency at the top.
          </Text>
        </VStack>
        <Grid columns={{ minWidth: 220 }} gap={3}>
          {field("first_name", "First name")}
          {field("last_name", "Last name", { required: true })}
        </Grid>
        {field("company", "Company")}
        {field("line1", "Address", { required: true })}
        {field("line2", "Apartment, suite")}
        <Grid columns={{ minWidth: 220 }} gap={3}>
          {field("postal_code", "Postal code")}
          {field("city", "City", { required: true })}
        </Grid>
        <Grid columns={{ minWidth: 220 }} gap={3}>
          {field("province", "Region")}
          {field("phone", "Phone")}
        </Grid>
      </VStack>
      {error && <Banner status="error" title={error} />}
      <div>
        <Button
          label={saved ? "Update details" : "Continue to shipping"}
          variant={saved ? "secondary" : "primary"}
          size="lg"
          isLoading={pending}
          clickAction={submit}
        />
      </div>
    </VStack>
  );
}

export type Offer = { option: string; label: string; price: string };

/** One group's shipping options; choosing one saves it. */
export function ShippingChoice({
  country,
  group,
  heading,
  offers,
  selected,
  problem,
}: {
  country: string;
  group: string;
  heading?: string;
  offers: Offer[];
  selected?: string;
  problem?: boolean;
}) {
  const [value, setValue] = useState(selected ?? "");
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();
  const choose = (option: string) => {
    setValue(option);
    start(async () => {
      const result = await selectShipping({ country, group, option });
      setError(result?.error);
      if (result) setValue(selected ?? "");
    });
  };

  if (offers.length === 0) {
    return <Banner status="warning" title="No way to ship this here yet." />;
  }
  return (
    <VStack gap={2}>
      {problem && <Banner status="warning" title="That option can't be used now: choose again." />}
      <RadioList
        label={heading ?? "Shipping method"}
        isLabelHidden={!heading}
        value={value}
        onChange={choose}
        isDisabled={pending}
      >
        {offers.map((o) => (
          <RadioListItem
            key={o.option}
            value={o.option}
            label={o.label}
            endContent={
              <Text type="body" weight="medium" hasTabularNumbers>
                {o.price}
              </Text>
            }
          />
        ))}
      </RadioList>
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}

export type Method = { id: string; name: string; instructions: string | null };

/** The payment step's id, for "Place order" to go back to. */
export const PAYMENT_STEP = "payment";

/** How the shopper pays: one of the cart's methods; choosing one saves it. The chosen
 * method's instructions (bank details) show under it. */
export function PaymentChoice({
  country,
  methods,
  selected,
  problem,
}: {
  country: string;
  methods: Method[];
  selected?: string;
  /** The cart's method isn't offered any more (disabled, or not in this market). */
  problem?: boolean;
}) {
  const [value, setValue] = useState(selected ?? "");
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();
  const choose = (method: string) => {
    setValue(method);
    start(async () => {
      const result = await selectPaymentMethod({ country, method });
      setError(result?.error);
      if (result) setValue(selected ?? "");
    });
  };

  return (
    <VStack gap={2}>
      {problem && (
        <Banner
          status="warning"
          title="The payment method you chose can't be used now: choose again."
        />
      )}
      <RadioList
        label="Payment method"
        isLabelHidden
        value={value}
        onChange={choose}
        isDisabled={pending}
      >
        {methods.map((m) => (
          <RadioListItem
            key={m.id}
            value={m.id}
            label={m.name}
            className="shop-option-top"
            description={
              m.id === value && m.instructions ? (
                <span className="shop-pre-line">{m.instructions}</span>
              ) : undefined
            }
          />
        ))}
      </RadioList>
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}

/** "Place order", at the total the shopper sees. */
export function PlaceOrder({
  country,
  total,
  attempt,
  ready,
  missing,
}: {
  country: string;
  total: { amount: number; currency: string };
  /** One per render of the checkout: a double click places one order. */
  attempt: string;
  ready: boolean;
  /** What's left to do before the order can be placed. */
  missing?: string;
}) {
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();
  return (
    <VStack gap={2}>
      <Button
        label="Place order"
        variant="primary"
        size="lg"
        width="100%"
        isDisabled={!ready}
        isLoading={pending}
        clickAction={() =>
          start(async () => {
            const result = await placeOrder({ country, total, attempt });
            if (result) setError(result.error);
            if (result?.step === "payment") {
              document.getElementById(PAYMENT_STEP)?.scrollIntoView({ behavior: "smooth" });
            }
          })
        }
      />
      {!ready && missing && <Text type="supporting">{missing}</Text>}
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}

/** How long the checkout waits for the provider to confirm a payment the shopper made. */
const CONFIRM_TRIES = 15;
const CONFIRM_DELAY_MS = 2000;

/**
 * Paying online before the order exists (design note 0031): "Continue to payment" starts
 * the cart's payment, the provider's card form (its storefront element, design note
 * 0029, §7) mounts here, and once it says the shopper paid the order is placed (asked
 * again for a while if the provider hasn't confirmed yet). A provider's page (a
 * redirect) comes back to `/checkout/complete`, which places the order.
 */
export function PayOnline({
  country,
  total,
  attempt,
  ready,
  missing,
  label,
}: {
  country: string;
  total: { amount: number; currency: string };
  attempt: string;
  ready: boolean;
  missing?: string;
  /** "Continue to payment" or the like. */
  label: string;
}) {
  const [started, setStarted] = useState<CartPaymentResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [waiting, setWaiting] = useState(false);
  const [pending, start] = useTransition();

  /** Places the order, asking again while the provider hasn't confirmed the payment. */
  const place = async () => {
    setWaiting(true);
    setError(null);
    for (let attemptNo = 0; attemptNo < CONFIRM_TRIES; attemptNo++) {
      // Success leaves this page (the action redirects to the confirmation).
      const result = await placeOrder({ country, total, attempt });
      if (!result?.pending) {
        setWaiting(false);
        if (result) setError(result.error);
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, CONFIRM_DELAY_MS));
    }
    setWaiting(false);
    setError(
      "Your payment is still being confirmed. You'll get an email once your order is placed.",
    );
  };

  const begin = () =>
    start(async () => {
      setError(null);
      const returnUrl = new URL(
        `${window.location.pathname.replace(/\/checkout\/?$/, "")}/checkout/complete`,
        window.location.origin,
      ).toString();
      const result = await startCartPayment({ country, total, returnUrl });
      if (result.committed) return place();
      const next = result.nextAction;
      if (next?.type === "redirect" && /^https?:\/\//.test(next.url)) {
        window.location.assign(next.url);
        return;
      }
      setStarted(result);
      setError(result.error);
    });

  const onEvent = (event: ElementEvent) => {
    if (event.type === "hypha:payment-confirmed") void place();
    else if (event.type === "hypha:error") setError(event.detail.message);
  };

  const next = started?.nextAction ?? null;
  return (
    <VStack gap={3}>
      {next?.type === "client_widget" && started?.module ? (
        <HyphaElement
          tag={next.element}
          module={started.module}
          props={next.props}
          onEvent={onEvent}
        />
      ) : (
        <Button
          label={started ? "Try again" : label}
          variant="primary"
          size="lg"
          width="100%"
          isDisabled={!ready}
          isLoading={pending}
          clickAction={begin}
        />
      )}
      {next?.type === "client_widget" && !started?.module && (
        <Text color="secondary">This payment can&apos;t be shown here. Try again later.</Text>
      )}
      {waiting && <Banner status="info" title="Confirming your payment…" />}
      {!ready && missing && <Text type="supporting">{missing}</Text>}
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}
