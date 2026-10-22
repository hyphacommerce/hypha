import { Banner } from "@astryxdesign/core/Banner";
import { Grid } from "@astryxdesign/core/Grid";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { lineImage } from "@/components/line-image";
import { SummaryLine, Totals } from "@/components/totals";
import { loadAccount } from "@/lib/account";
import { api, countryOf } from "@/lib/api";
import { formatMoney } from "@/lib/money";
import { cartId, link } from "@/lib/site";

import {
  CheckoutPayment,
  DetailsForm,
  OnlinePaymentForm,
  PAYMENT_STEP,
  PayButton,
  PaymentChoice,
  PlaceOrder,
  ShippingChoice,
} from "./checkout-forms";

/** Per request: it reads the shopper's cart or query. */
export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Checkout" };

/**
 * Checkout on one page: contact and shipping address, a shipping option per
 * group, how the shopper pays when there's something to pay, and a summary with "Place
 * order", or, for a method paid online, the payment itself: the order is placed once
 * it's paid.
 */
export default async function CheckoutPage({
  params,
  searchParams,
}: {
  params: Promise<{ country: string }>;
  searchParams: Promise<{ payment?: string | string[] }>;
}) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const digits = country.minor_units;
  const id = await cartId(code);
  if (!id) redirect(link(code, "/cart"));
  const client = api();
  // The options alongside the cart, not after it: they only count once the cart has an
  // address and something to ship, and otherwise the API refuses them (unused here).
  const [{ data: cart }, options, payment, account] = await Promise.all([
    client.GET("/storefront/carts/{id}", { params: { path: { id } } }),
    client.GET("/storefront/carts/{id}/shipping-options", { params: { path: { id } } }),
    client.GET("/storefront/carts/{id}/payment-methods", { params: { path: { id } } }),
    // A signed-in shopper's address book and marketing choice fill the form.
    loadAccount(),
  ]);
  if (!cart || cart.order || cart.lines.length === 0) redirect(link(code, "/cart"));

  const address = cart.shipping_address;
  // Their shipping address in this country, or the first one here.
  const here = account?.addresses.filter((a) => a.country === code) ?? [];
  const saved = here.find((a) => a.id === account?.default_shipping_address) ?? here[0];
  // Prefilled from the address book until the cart has its own.
  const prefill = address ?? saved ?? null;
  const ships = cart.shipping_groups.length > 0;
  const offers = ships && address ? (options.data?.groups ?? []) : [];
  const detailsDone = Boolean(cart.email) && (!ships || Boolean(address));
  const shippingDone = !ships || cart.shipping_ready;
  // A free order has nothing to pay, so no payment step.
  const pays = cart.total.amount > 0;
  const methods = payment.data?.items ?? [];
  // The cart's choice counts only while it's still offered.
  const method = methods.find((m) => m.id === cart.payment_method);
  const paymentDone = !pays || Boolean(method);
  // Paid online before the order: the card form on this page.
  const online = pays && method !== undefined && method.provider !== "manual";
  // Back from a provider's page without an order (`/checkout/complete`): why.
  const returned = (await searchParams).payment;
  const returnError = typeof returned === "string" ? returned : undefined;
  // Nothing to pay with: the store can't take this order (as Shopify says it).
  const closed = pays && payment.data !== undefined && methods.length === 0;
  const ready = detailsDone && cart.fully_available && shippingDone && paymentDone;
  const missing = !detailsDone
    ? "Enter your details first."
    : !cart.fully_available
      ? "Something in your cart can't be bought: check your cart."
      : !shippingDone
        ? "Choose how your order ships."
        : "Choose how you'll pay.";
  // The "Place order" key, one per render of this page (`PlaceOrder`): made here on the
  // server, per request, and sent down as a prop, so the browser never makes its own.
  const attempt = crypto.randomUUID();

  const total = { amount: cart.total.amount, currency: cart.total.currency };
  return (
    <CheckoutPayment country={code} total={total} attempt={attempt}>
      <VStack gap={6}>
        <Heading level={1} type="display-2">
          Checkout
        </Heading>
        <Grid columns={{ minWidth: 360, repeat: "fit" }} gap={8}>
          <VStack gap={8}>
            <DetailsForm
              country={code}
              countryName={country.name}
              saved={detailsDone}
              initialOptIn={
                cart.marketing_opt_in || account?.email_marketing.state === "subscribed"
              }
              emailFixed={Boolean(account) && cart.customer === account?.id}
              initial={{
                email: cart.email ?? account?.email ?? "",
                first_name: prefill?.first_name ?? "",
                last_name: prefill?.last_name ?? "",
                company: prefill?.company ?? "",
                line1: prefill?.line1 ?? "",
                line2: prefill?.line2 ?? "",
                postal_code: prefill?.postal_code ?? "",
                city: prefill?.city ?? "",
                province: prefill?.province ?? "",
                phone: prefill?.phone ?? "",
              }}
            />
            {ships && (
              <VStack gap={3}>
                <Divider />
                <Text type="large" weight="bold">
                  Delivery
                </Text>
                {!address ? (
                  <Text color="secondary">Enter your address to see the options.</Text>
                ) : (
                  offers.map((group, index) => {
                    const chosen = cart.shipping_groups.find((g) => g.id === group.group);
                    return (
                      <ShippingChoice
                        key={group.group}
                        country={code}
                        group={group.group}
                        heading={offers.length > 1 ? `Shipment ${index + 1}` : undefined}
                        selected={chosen?.selected?.option}
                        problem={Boolean(chosen?.problem)}
                        offers={group.offers.map((offer) => ({
                          option: offer.option,
                          label: offer.estimate
                            ? `${offer.name} (${offer.estimate.min_days}–${offer.estimate.max_days} days)`
                            : offer.name,
                          price: formatMoney(offer.amount, digits),
                        }))}
                      />
                    );
                  })
                )}
              </VStack>
            )}
            {pays && (
              <div id={PAYMENT_STEP}>
                <VStack gap={3}>
                  <Divider />
                  <Text type="large" weight="bold">
                    Payment
                  </Text>
                  {closed ? (
                    <Text color="secondary">This store can't take orders right now.</Text>
                  ) : !detailsDone || !shippingDone ? (
                    <Text color="secondary">
                      {detailsDone
                        ? "Choose how your order ships first."
                        : "Enter your details first."}
                    </Text>
                  ) : (
                    <PaymentChoice
                      country={code}
                      methods={methods.map((m) => ({
                        id: m.id,
                        name: m.name,
                        instructions: m.instructions ?? null,
                      }))}
                      selected={method?.id}
                      problem={Boolean(cart.payment_method) && !method}
                      detail={
                        online && ready ? (
                          <OnlinePaymentForm
                            // A new method or total starts a new payment.
                            key={`${method?.id}:${cart.total.amount}`}
                            country={code}
                            total={total}
                          />
                        ) : undefined
                      }
                    />
                  )}
                </VStack>
              </div>
            )}
          </VStack>

          <div className="shop-sticky">
            <Card padding={5}>
              <VStack gap={4}>
                <Heading level={2}>Order summary</Heading>
                {cart.lines.map((line) => (
                  <SummaryLine
                    key={line.variant}
                    quantity={line.quantity}
                    title={line.product_title ?? "A product no longer sold"}
                    image={lineImage(line.image)}
                    variant={line.variant_title}
                    total={line.status === "priced" ? formatMoney(line.total, digits) : "—"}
                  />
                ))}
                <Divider />
                <Totals
                  subtotal={cart.subtotal}
                  shipping={cart.shipping_total}
                  tax={cart.tax_total}
                  total={cart.total}
                  taxesIncluded={cart.taxes_included}
                  digits={digits}
                />
                {returnError && <Banner status="error" title={returnError} />}
                {closed ? (
                  <Banner
                    status="warning"
                    title="This store can't take orders right now"
                    description="There's no way to pay for this order yet. Please try again later."
                  />
                ) : online ? (
                  <PayButton
                    ready={ready}
                    missing={missing}
                    label={`Pay ${formatMoney(cart.total, digits)}`}
                  />
                ) : (
                  <PlaceOrder
                    country={code}
                    total={{ amount: cart.total.amount, currency: cart.total.currency }}
                    attempt={attempt}
                    ready={ready}
                    missing={missing}
                  />
                )}
              </VStack>
            </Card>
          </div>
        </Grid>
      </VStack>
    </CheckoutPayment>
  );
}
