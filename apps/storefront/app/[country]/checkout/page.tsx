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
import { api, countryOf } from "@/lib/api";
import { formatMoney } from "@/lib/money";
import { cartId, link } from "@/lib/site";

import { DetailsForm, PlaceOrder, ShippingChoice } from "./checkout-forms";

/** Per request: it reads the shopper's cart or query (design note 0024, §7). */
export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Checkout" };

/**
 * Checkout (design notes 0023, §3, and 0024, §7) on one page: contact and shipping
 * address, a shipping option per group (0015), and a summary with "Place order",
 * unpaid (0021, SU8).
 */
export default async function CheckoutPage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const digits = country.minor_units;
  const id = await cartId(code);
  if (!id) redirect(link(code, "/cart"));
  const client = api();
  // The options alongside the cart, not after it: they only count once the cart has an
  // address and something to ship, and otherwise the API refuses them (unused here).
  const [{ data: cart }, options] = await Promise.all([
    client.GET("/storefront/carts/{id}", { params: { path: { id } } }),
    client.GET("/storefront/carts/{id}/shipping-options", { params: { path: { id } } }),
  ]);
  if (!cart || cart.order || cart.lines.length === 0) redirect(link(code, "/cart"));

  const address = cart.shipping_address;
  const ships = cart.shipping_groups.length > 0;
  const offers = ships && address ? (options.data?.groups ?? []) : [];
  const detailsDone = Boolean(cart.email) && (!ships || Boolean(address));
  const ready = detailsDone && cart.fully_available && (!ships || cart.shipping_ready);
  const missing = !detailsDone
    ? "Enter your details first."
    : !cart.fully_available
      ? "Something in your cart can't be bought: check your cart."
      : "Choose how your order ships.";

  return (
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
            initial={{
              email: cart.email ?? "",
              first_name: address?.first_name ?? "",
              last_name: address?.last_name ?? "",
              company: address?.company ?? "",
              line1: address?.line1 ?? "",
              line2: address?.line2 ?? "",
              postal_code: address?.postal_code ?? "",
              city: address?.city ?? "",
              province: address?.province ?? "",
              phone: address?.phone ?? "",
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
              <PlaceOrder
                country={code}
                total={{ amount: cart.total.amount, currency: cart.total.currency }}
                attempt={crypto.randomUUID()}
                ready={ready}
                missing={missing}
              />
            </VStack>
          </Card>
        </div>
      </Grid>
    </VStack>
  );
}
