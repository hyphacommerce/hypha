"use client";

// Checkout's forms (design note 0024, §7; the `payment-form` template without its card
// section, since orders are placed unpaid, 0021 SU8): contact and address, a shipping
// option per group, and "Place order". Each saves through its server action, and the
// page re-renders from the cart the API answers with.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Grid } from "@astryxdesign/core/Grid";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import {
  type Details,
  placeOrder,
  saveDetails,
  selectShipping,
} from "@/app/[country]/actions";

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
  const router = useRouter();
  const [details, setDetails] = useState(initial);
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();
  const [tried, setTried] = useState(false);

  const set = (key: keyof Details) => (value: string) =>
    setDetails((d) => ({ ...d, [key]: value }));
  const required = (key: keyof Details) =>
    tried && details[key].trim() === "" ? { type: "error" as const, message: "Required" } : undefined;

  const submit = () => {
    setTried(true);
    if (["email", "last_name", "line1", "city"].some((k) => details[k as keyof Details].trim() === "")) {
      return;
    }
    start(async () => {
      const result = await saveDetails(country, details);
      setError(result?.error);
      if (!result) router.refresh();
    });
  };

  const field = (key: keyof Details, label: string, options: { required?: boolean; type?: "email" } = {}) => (
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
            Ships to {countryName}. To ship elsewhere, change the country at the top.
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
  const router = useRouter();
  const [value, setValue] = useState(selected ?? "");
  const [error, setError] = useState<string>();
  const [pending, start] = useTransition();
  const choose = (option: string) => {
    setValue(option);
    start(async () => {
      const result = await selectShipping({ country, group, option });
      setError(result?.error);
      if (result) setValue(selected ?? "");
      else router.refresh();
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
          })
        }
      />
      {!ready && missing && <Text type="supporting">{missing}</Text>}
      {error && <Banner status="error" title={error} />}
      <Text type="supporting">You pay on delivery: no payment is taken online.</Text>
    </VStack>
  );
}
