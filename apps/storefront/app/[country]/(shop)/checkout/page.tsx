import { Banner } from "@astryxdesign/core/Banner";
import { Grid } from "@astryxdesign/core/Grid";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { CartMessages } from "@/components/cart-messages";
import { FollowCart } from "@/components/cart/cart-provider";
import { CodeStates } from "@/components/discounts";
import { lineImage } from "@/components/line-image";
import { SummaryLine, Totals } from "@/components/totals";
import { loadAccount } from "@/lib/account";
import { api, countryOf, loadAddressFormat, pageLocale } from "@/lib/api";
import { readCart } from "@/lib/cart";
import { Price } from "@/components/price";
import { linePrice, readCartDiscounts, shippingPrice } from "@/lib/discounts";
import { placeMessages } from "@/lib/cart-messages";
import { countryName } from "@/lib/format";
import { pageT } from "@/lib/i18n";
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

export async function generateMetadata({
  params,
}: {
  params: Promise<{ country: string }>;
}): Promise<Metadata> {
  const t = await pageT((await params).country);
  return { title: t("checkout.meta_title") };
}

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
  const folder = (await params).country;
  const [country, place, t] = await Promise.all([
    countryOf(folder),
    pageLocale(folder),
    pageT(folder),
  ]);
  const code = country.country;
  const digits = country.minor_units;
  const id = await cartId(code);
  if (!id) redirect(link(folder, "/cart"));
  const client = api();
  // The options and methods are named in the cart's language.
  const offered = (init: { headers?: Record<string, string> } = {}) =>
    Promise.all([
      client.GET("/storefront/carts/{id}/shipping-options", { params: { path: { id } }, ...init }),
      client.GET("/storefront/carts/{id}/payment-methods", { params: { path: { id } }, ...init }),
    ]);
  // The options alongside the cart, not after it: they only count once the cart has an
  // address and something to ship, and otherwise the API refuses them (unused here).
  const [read, alongside, account, format] = await Promise.all([
    readCart(id, place),
    offered(),
    // A signed-in shopper's address book and marketing choice fill the form.
    loadAccount(),
    // How the country lays out its addresses.
    loadAddressFormat(code),
  ]);
  // The read moved the cart to the page's language: what came alongside was in the old.
  // A render answers a GET it already sent from the first answer (fetch deduplication,
  // keyed by URL and headers; a signal doesn't reach it through the client's `Request`),
  // so the second read differs by a header.
  const [options, payment] = read.moved
    ? await offered({ headers: { "cache-control": "no-cache" } })
    : alongside;
  const cart = read.cart;
  if (!cart || cart.order || cart.lines.length === 0) redirect(link(folder, "/cart"));

  const discounts = readCartDiscounts(cart);
  // Each under what it's about: errors stop "Place order" (`checkout_blocked`).
  const messages = placeMessages(cart.messages);
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
    ? t("checkout.details_first")
    : !cart.fully_available
      ? t("checkout.cart_unavailable")
      : !shippingDone
        ? t("checkout.choose_shipping")
        : t("checkout.choose_payment");
  // The "Place order" key, one per render of this page (`PlaceOrder`): made here on the
  // server, per request, and sent down as a prop, so the browser never makes its own.
  const attempt = crypto.randomUUID();

  const total = { amount: cart.total.amount, currency: cart.total.currency };
  return (
    <CheckoutPayment total={total} attempt={attempt}>
      {/* The drawer changes the cart too: this page re-renders with what it did. */}
      <FollowCart />
      <VStack gap={6}>
        <Heading level={1} type="display-2">
          {t("checkout.title")}
        </Heading>
        <CartMessages t={t} messages={messages.top} />
        <Grid columns={{ minWidth: 360, repeat: "fit" }} gap={8}>
          <VStack gap={8}>
            <DetailsForm
              countryName={countryName(code, country.name, t.locale)}
              format={format}
              saved={detailsDone}
              messages={messages}
              suggestion={
                address ? (cart.shipping_address_check?.suggested ?? undefined) : undefined
              }
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
                  {t("checkout.delivery")}
                </Text>
                {!address ? (
                  <Text color="secondary">{t("checkout.address_first")}</Text>
                ) : (
                  offers.map((group, index) => {
                    const at = cart.shipping_groups.findIndex((g) => g.id === group.group);
                    const chosen = cart.shipping_groups[at];
                    return (
                      <ShippingChoice
                        key={group.group}
                        group={group.group}
                        heading={
                          offers.length > 1
                            ? t("checkout.shipment_number", { number: index + 1 })
                            : undefined
                        }
                        selected={chosen?.selected?.option}
                        problem={Boolean(chosen?.problem)}
                        messages={messages.groups[at]}
                        offers={group.offers.map((offer) => {
                          // "Free" when a shipping discount takes all of it.
                          const price = shippingPrice(
                            t,
                            offer.amount_after_discounts,
                            offer.amount,
                            digits,
                          );
                          return {
                            option: offer.option,
                            label: offer.estimate
                              ? t("checkout.offer_estimate", {
                                  name: offer.name,
                                  min: offer.estimate.min_days,
                                  max: offer.estimate.max_days,
                                })
                              : offer.name,
                            price: price.text,
                            was: price.was,
                          };
                        })}
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
                    {t("checkout.payment")}
                  </Text>
                  {closed ? (
                    <Text color="secondary">{t("checkout.store_closed")}</Text>
                  ) : !detailsDone || !shippingDone ? (
                    <Text color="secondary">
                      {detailsDone ? t("checkout.shipping_first") : t("checkout.details_first")}
                    </Text>
                  ) : (
                    <PaymentChoice
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
                <Heading level={2}>{t("checkout.order_summary")}</Heading>
                {cart.lines.map((line, index) => {
                  const shown =
                    line.status === "priced"
                      ? linePrice(
                          t,
                          line,
                          discounts.lines[line.variant],
                          discounts.applications,
                          digits,
                        )
                      : null;
                  return (
                    <VStack key={line.variant} gap={2}>
                      <SummaryLine
                        t={t}
                        quantity={line.quantity}
                        title={line.product_title ?? t("common.product_gone")}
                        image={lineImage(line.image)}
                        variant={line.variant_title}
                        notes={shown?.notes}
                        total={
                          shown ? (
                            <Price
                              t={t}
                              price={shown.price}
                              compareAt={shown.compareAt}
                              digits={digits}
                            />
                          ) : (
                            "—"
                          )
                        }
                      />
                      <CartMessages t={t} messages={messages.lines[index]} />
                    </VStack>
                  );
                })}
                <CodeStates
                  t={t}
                  codes={discounts.codes}
                  applications={discounts.applications}
                  digits={digits}
                />
                <Divider />
                <Totals
                  t={t}
                  subtotal={cart.subtotal}
                  shipping={cart.shipping_total}
                  tax={cart.tax_total}
                  total={cart.total}
                  taxesIncluded={cart.taxes_included}
                  digits={digits}
                  discounts={discounts}
                />
                {returnError && <Banner status="error" title={returnError} />}
                {closed ? (
                  <Banner
                    status="warning"
                    title={t("checkout.store_closed_title")}
                    description={t("checkout.store_closed_description")}
                  />
                ) : online ? (
                  <PayButton
                    ready={ready}
                    missing={missing}
                    label={t("checkout.pay", {
                      total: formatMoney(cart.total, digits, t.locale),
                    })}
                  />
                ) : (
                  <PlaceOrder
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
