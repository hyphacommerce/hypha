"use client";

// Finishing an order's payment (design note 0029, §7; docs/apps/elements.md): a payment
// app's `client_widget` is its custom element, mounted here with the props the app
// answered. `hypha:payment-confirmed` means the shopper paid (the order's payment moves
// when the app reports it); `hypha:error` shows the app's message with a retry, as does
// a start that failed. Retrying starts the payment again (`startPayment`).

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/Stack";
import { type ElementEvent, HyphaElement } from "@hypha/storefront-elements/react";
import { useState, useTransition } from "react";

import { type PaymentStartResult, paymentConfirmed, startPayment } from "@/app/[country]/actions";

export function CompletePayment({
  order,
  initial,
}: {
  order: string;
  /** How checkout started it, when it did; without one, a button starts it. */
  initial: PaymentStartResult | null;
}) {
  const [start, setStart] = useState(initial);
  const [error, setError] = useState(initial?.error ?? null);
  const [confirmed, setConfirmed] = useState(false);
  const [pending, startTransition] = useTransition();

  const retry = () =>
    startTransition(async () => {
      setError(null);
      const next = await startPayment({ order });
      setStart(next);
      setError(next.error);
    });

  const onEvent = (event: ElementEvent) => {
    if (event.type === "hypha:payment-confirmed") {
      setConfirmed(true);
      void paymentConfirmed({ order });
    } else if (event.type === "hypha:error") {
      setError(event.detail.message);
    }
  };

  if (confirmed) {
    return (
      <Banner
        status="success"
        title="Payment received"
        description="Thank you. We'll email you once it's confirmed."
      />
    );
  }

  const next = start?.nextAction ?? null;
  return (
    <VStack gap={3}>
      {error && (
        <Banner
          status="error"
          title={start?.nextAction ? "The payment didn't go through" : "Payment couldn't start"}
          description={error}
        />
      )}
      {next?.type === "client_widget" &&
        (start?.module ? (
          <HyphaElement
            tag={next.element}
            module={start.module}
            props={next.props}
            onEvent={onEvent}
          />
        ) : (
          <Text color="secondary">This payment can&apos;t be shown here. Try again later.</Text>
        ))}
      {next?.type === "instructions" && (
        <Text color="secondary">
          <span className="shop-pre-line">{next.instructions}</span>
        </Text>
      )}
      {next?.type === "redirect" && /^https?:\/\//.test(next.url) && (
        <div>
          <Button
            label="Continue to payment"
            variant="primary"
            onClick={() => window.location.assign(next.url)}
          />
        </div>
      )}
      {(!next || error) && (
        <div>
          <Button
            label={start ? "Try again" : "Complete payment"}
            variant={next ? "secondary" : "primary"}
            isLoading={pending}
            onClick={retry}
          />
        </div>
      )}
    </VStack>
  );
}
