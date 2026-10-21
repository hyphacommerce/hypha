"use client";

// Checkout's forms (design note 0024, §7): contact and address, a shipping option per
// group, how the shopper pays, and "Place order" (a manual method) or the payment
// itself (an online one, paid before the order exists: design note 0031). Each saves
// through its server action, which answers with the page re-rendered from the cart
// (`refresh()`).

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Grid } from "@astryxdesign/core/Grid";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type ElementEvent, HyphaElement } from "@hypha/storefront-elements/react";
import { createContext, useContext, useEffect, useMemo, useState, useTransition } from "react";

import {
  type CartPaymentResult,
  type Details,
  placeOrder,
  saveDetails,
  selectPaymentMethod,
  selectShipping,
  startCartPayment,
} from "@/app/[country]/actions";
import { Skeleton } from "@/components/skeletons";

/** The fields the form won't save without (marked required below). */
const REQUIRED: (keyof Details)[] = ["email", "last_name", "line1", "city"];

/** Contact and shipping address, saved together. */
export function DetailsForm({
  country,
  countryName,
  initial,
  initialOptIn,
  saved,
}: {
  country: string;
  countryName: string;
  initial: Details;
  /** Whether the shopper ticked "Email me with news and offers" (design note 0032, §5). */
  initialOptIn: boolean;
  /** Whether the cart already has them (the button says "Update"). */
  saved: boolean;
}) {
  const [details, setDetails] = useState(initial);
  const [optIn, setOptIn] = useState(initialOptIn);
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
      const result = await saveDetails(country, details, optIn);
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
        <CheckboxInput label="Email me with news and offers" value={optIn} onChange={setOptIn} />
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
        containerClassName="shop-choice-cards"
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

/** How the shopper pays: one of the cart's methods, each a wide card; choosing one
 * saves it. The chosen method's card holds its instructions (bank details) or, paid
 * online, its payment form (`detail`), as Shopify's checkout does. */
export function PaymentChoice({
  country,
  methods,
  selected,
  problem,
  detail,
}: {
  country: string;
  methods: Method[];
  selected?: string;
  /** The cart's method isn't offered any more (disabled, or not in this market). */
  problem?: boolean;
  /** What the saved method's card holds below its name (the online payment form). */
  detail?: React.ReactNode;
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
        containerClassName="shop-choice-cards"
        value={value}
        onChange={choose}
        isDisabled={pending}
      >
        {methods.map((m) => {
          const chosen = m.id === value;
          // The form only once the cart has the method (the server sends it then).
          const form = chosen && m.id === selected ? detail : undefined;
          return (
            <RadioListItem
              key={m.id}
              value={m.id}
              label={<Text weight="medium">{m.name}</Text>}
              className="shop-option-top"
              description={
                chosen && (m.instructions || form) ? (
                  <>
                    {m.instructions && <span className="shop-pre-line">{m.instructions}</span>}
                    {form && <ChoiceDetail>{form}</ChoiceDetail>}
                  </>
                ) : undefined
              }
            />
          );
        })}
      </RadioList>
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}

/**
 * A form inside a chosen option's card. The card hands clicks on its surface to its
 * radio (which would take focus from the form's fields), so clicks here stop.
 */
function ChoiceDetail({ children }: { children: React.ReactNode }) {
  return (
    // oxlint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions -- not a control: it only keeps the card's click delegation out of the form
    <div className="shop-choice-detail" onClick={(event) => event.stopPropagation()}>
      {children}
    </div>
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
 * What the checkout's payment form (in the payment step) and its "Pay" button (in the
 * summary) share (design note 0031): Shopify's way, the provider's form appears once the
 * method is chosen and the summary's one button pays with it.
 */
type OnlinePayment = {
  /** The mounted payment element, for `submit()`. */
  element: HTMLElement | null;
  setElement: (element: HTMLElement | null) => void;
  /** The form can be submitted. */
  ready: boolean;
  setReady: (ready: boolean) => void;
  /** A provider's page to send the shopper to instead (a redirect method). */
  redirect: string | null;
  setRedirect: (url: string | null) => void;
  /** The cart's money is committed already (a reload after paying): just place the order. */
  committed: boolean;
  setCommitted: (committed: boolean) => void;
  /** Paying (or placing the order after): the button waits. */
  paying: boolean;
  setPaying: (paying: boolean) => void;
  /** What the shopper should see, under the form and the button. */
  error: string | null;
  setError: (error: string | null) => void;
  /** Places the order once the money is committed (asked again while it's confirmed). */
  place: () => Promise<void>;
};

const PaymentContext = createContext<OnlinePayment | null>(null);

function usePayment(): OnlinePayment {
  const payment = useContext(PaymentContext);
  if (!payment) throw new Error("the payment form and button need <CheckoutPayment>");
  return payment;
}

/** The checkout's payment state, for the form and the button it holds. */
export function CheckoutPayment({
  country,
  total,
  attempt,
  children,
}: {
  country: string;
  total: { amount: number; currency: string };
  attempt: string;
  children: React.ReactNode;
}) {
  const [element, setElement] = useState<HTMLElement | null>(null);
  const [ready, setReady] = useState(false);
  const [redirect, setRedirect] = useState<string | null>(null);
  const [committed, setCommitted] = useState(false);
  const [paying, setPaying] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { amount, currency } = total;
  const value = useMemo<OnlinePayment>(() => {
    const place = async () => {
      setPaying(true);
      setError(null);
      for (let tries = 0; tries < CONFIRM_TRIES; tries++) {
        // Success leaves this page (the action redirects to the confirmation).
        const result = await placeOrder({ country, total: { amount, currency }, attempt });
        if (!result?.pending) {
          setPaying(false);
          if (result) setError(result.error);
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, CONFIRM_DELAY_MS));
      }
      setPaying(false);
      setError(
        "Your payment is still being confirmed. You'll get an email once your order is placed.",
      );
    };
    return {
      element,
      setElement: (next) => {
        setElement(next);
        if (!next) setReady(false);
      },
      ready,
      setReady,
      redirect,
      setRedirect,
      committed,
      setCommitted,
      paying,
      setPaying,
      error,
      setError,
      place,
    };
  }, [element, ready, redirect, committed, paying, error, country, amount, currency, attempt]);
  return <PaymentContext.Provider value={value}>{children}</PaymentContext.Provider>;
}

/**
 * The online method's payment form, under the method once it's chosen and the cart is
 * ready (design note 0031): it starts the cart's payment and mounts the provider's
 * element (its own button hidden: the summary's "Pay" pays). Restarted (by its `key`)
 * when the method or the total changes.
 */
export function OnlinePaymentForm({
  country,
  total,
}: {
  country: string;
  total: { amount: number; currency: string };
}) {
  const payment = usePayment();
  const [started, setStarted] = useState<CartPaymentResult | null>(null);
  const [round, setRound] = useState(0);
  // The provider's form said it's ready (`hypha:ready`): until then, its skeleton.
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let current = true;
    const run = async () => {
      const returnUrl = new URL(
        `${window.location.pathname.replace(/\/checkout\/?$/, "")}/checkout/complete`,
        window.location.origin,
      ).toString();
      // The guard after the answer is the point: a form gone meanwhile drops it.
      // react-doctor-disable-next-line react-doctor/async-defer-await -- see above
      const result = await startCartPayment({ country, total, returnUrl });
      if (!current) return;
      setStarted(result);
      payment.setCommitted(result.committed);
      payment.setError(result.error);
      const next = result.nextAction;
      payment.setRedirect(
        next?.type === "redirect" && /^https?:\/\//.test(next.url) ? next.url : null,
      );
    };
    void run();
    return () => {
      current = false;
    };
    // Started once per mount (and per "Try again"): the key restarts it for a new total.
    // oxlint-disable-next-line react-hooks/exhaustive-deps -- `payment`'s setters are stable in intent; re-running on every render would start a payment each time
  }, [round]);

  const onEvent = (event: ElementEvent) => {
    if (event.type === "hypha:ready") {
      setLoaded(true);
      payment.setReady(true);
    } else if (event.type === "hypha:payment-confirmed") void payment.place();
    else if (event.type === "hypha:error") {
      payment.setPaying(false);
      payment.setError(event.detail.message);
    }
  };

  if (!started) return <PaymentFormSkeleton />;
  const next = started.nextAction;
  if (next?.type === "client_widget") {
    if (!started.module) {
      return <Text color="secondary">This payment can&apos;t be shown here. Try again later.</Text>;
    }
    // A failure before it's ready (Stripe.js didn't load) shows under the button.
    const waiting = !loaded && !payment.error;
    return (
      <div className="shop-payment-element" data-loading={waiting ? "" : undefined}>
        {waiting && <PaymentFormSkeleton />}
        <HyphaElement
          tag={next.element}
          module={started.module}
          // The summary's button pays (`submit()`), not the element's own.
          props={{ ...next.props, submit_mode: "external" }}
          onEvent={onEvent}
          onElement={payment.setElement}
        />
      </div>
    );
  }
  if (next?.type === "redirect") {
    return <Text color="secondary">You'll finish paying on the provider's page.</Text>;
  }
  if (started.committed) return <Text color="secondary">Your payment is in. Place the order.</Text>;
  return (
    <VStack gap={2}>
      {started.error && <Banner status="error" title={started.error} />}
      <div>
        <Button
          label="Try again"
          variant="secondary"
          clickAction={() => {
            setStarted(null);
            setRound((r) => r + 1);
          }}
        />
      </div>
    </VStack>
  );
}

/** One of the skeleton's fields: its label, then its input. */
function FieldSkeleton({ index, label }: { index: number; label: string }) {
  return (
    <VStack gap={2}>
      <Skeleton index={index} height={12} width={label} radius={1} />
      <Skeleton index={index} height={44} radius={2} />
    </VStack>
  );
}

/** A card form's outline while it loads: the methods' tabs, then number, expiry and
 * security code, and country, as the provider's form lays them out. */
function PaymentFormSkeleton() {
  return (
    <output className="shop-payment-skeleton" aria-label="Loading the payment form">
      <VStack gap={4}>
        <Grid columns={3} gap={3}>
          {[0, 1, 2].map((i) => (
            <Skeleton key={i} index={i} height={64} radius={2} />
          ))}
        </Grid>
        <FieldSkeleton index={3} label="30%" />
        <Grid columns={2} gap={3}>
          <FieldSkeleton index={4} label="45%" />
          <FieldSkeleton index={4} label="55%" />
        </Grid>
        <FieldSkeleton index={5} label="25%" />
      </VStack>
    </output>
  );
}

/**
 * The summary's "Pay" for an online method: submits the payment form (its outcome comes
 * back as the element's events: the order is placed once it's paid, a decline shows
 * here, and the shopper tries again), goes to a redirect method's page, or places the
 * order for money committed already.
 */
export function PayButton({
  ready,
  missing,
  label,
}: {
  /** The cart is ready (details, shipping, lines). */
  ready: boolean;
  missing?: string;
  label: string;
}) {
  const payment = usePayment();
  const canPay = ready && (payment.ready || payment.redirect !== null || payment.committed);
  const pay = () => {
    payment.setError(null);
    if (payment.redirect) {
      window.location.assign(payment.redirect);
      return;
    }
    if (payment.committed) {
      void payment.place();
      return;
    }
    const element: unknown = payment.element;
    const submit: unknown =
      element && typeof element === "object" ? Reflect.get(element, "submit") : undefined;
    if (typeof submit !== "function") return;
    payment.setPaying(true);
    Reflect.apply(submit, element, []);
  };
  return (
    <VStack gap={2}>
      <Button
        label={payment.paying ? "Processing…" : label}
        variant="primary"
        size="lg"
        width="100%"
        isDisabled={!canPay}
        isLoading={payment.paying}
        clickAction={pay}
      />
      {!ready && missing && <Text type="supporting">{missing}</Text>}
      {ready && !canPay && !payment.error && (
        <Text type="supporting">Enter your payment details above.</Text>
      )}
      {payment.error && <Banner status="error" title={payment.error} />}
    </VStack>
  );
}
