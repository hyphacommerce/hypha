// An order as its buyer sees it: the lines and totals, when it was placed and
// how it ships, the payment, the address. The confirmation page and the account's
// order page share it.

import { Badge } from "@astryxdesign/core/Badge";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { Grid } from "@astryxdesign/core/Grid";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

import type { StorefrontSchemas } from "@hyphacommerce/sdk/storefront";

import { AppElements } from "@/components/app-element";
import { lineImage } from "@/components/line-image";
import { Price } from "@/components/price";
import { SummaryLine, Totals } from "@/components/totals";
import { type Country } from "@/lib/api";
import { formatMoney } from "@/lib/money";
import { elementsFor } from "@/lib/app-elements";
import { lineCompareAt } from "@/lib/prices";

export type PlacedOrder = StorefrontSchemas["PlacedOrderResponse"];
type PaymentStatus = StorefrontSchemas["PaymentStatusDto"];

/** A payment's status for the buyer (an unknown one shows as it is: the enum is open). */
function paymentStatus(status: PaymentStatus): {
  label: string;
  variant: "success" | "warning" | "error" | "neutral";
} {
  switch (status) {
    case "pending":
      return { label: "Awaiting payment", variant: "warning" };
    case "authorized":
      return { label: "Authorized", variant: "neutral" };
    case "processing":
      return { label: "Processing", variant: "neutral" };
    case "paid":
      return { label: "Paid", variant: "success" };
    case "failed":
      return { label: "Payment failed", variant: "error" };
    case "voided":
      return { label: "Cancelled", variant: "neutral" };
    case "partially_refunded":
      return { label: "Partly refunded", variant: "neutral" };
    case "refunded":
      return { label: "Refunded", variant: "neutral" };
    default:
      return { label: status, variant: "neutral" };
  }
}

/** When the order was placed: "18 October 2026". */
export const PLACED_DATE = new Intl.DateTimeFormat("en-GB", { dateStyle: "long" });

/** The order's details, as two columns; `after` goes under them (a button). */
export async function OrderDetails({
  order,
  country,
  after,
}: {
  order: PlacedOrder;
  country: Country;
  after?: React.ReactNode;
}) {
  const digits = country.minor_units;
  const to = order.shipping_address;
  const payment = order.payment ?? null;
  // An order paid online was paid before it existed; one paid by hand shows its
  // instructions (bank transfer, cash on delivery).
  const confirmationElements = await elementsFor("order.confirmation");
  const status = payment ? paymentStatus(payment.status) : undefined;
  const placed = PLACED_DATE.format(new Date(order.placed_at));
  return (
    <>
      <Grid columns={{ minWidth: 320, repeat: "fit" }} gap={6}>
        <Card padding={5}>
          <VStack gap={4}>
            <Heading level={2}>Your order</Heading>
            {order.lines.map((line) => (
              <SummaryLine
                key={line.id}
                quantity={line.quantity}
                title={line.product_title}
                image={lineImage(line.image)}
                variant={line.variant_title}
                total={
                  <Price
                    price={line.total}
                    compareAt={lineCompareAt(line.compare_at_price, line.quantity, line.total)}
                    digits={digits}
                  />
                }
              />
            ))}
            <Divider />
            <Totals
              subtotal={order.subtotal}
              shipping={order.shipping_total}
              tax={order.tax_total}
              total={order.total}
              taxesIncluded={order.taxes_included}
              digits={digits}
            />
          </VStack>
        </Card>
        <VStack gap={4}>
          <Card padding={5}>
            <VStack gap={2}>
              <Heading level={2}>Details</Heading>
              <Text color="secondary">Placed {placed}</Text>
              {order.shipping_lines.map((s) => (
                <Text key={s.group} color="secondary">
                  {s.name}
                  {s.estimate ? `, ${s.estimate.min_days}–${s.estimate.max_days} days` : ""}
                </Text>
              ))}
            </VStack>
          </Card>
          <Card padding={5}>
            <VStack gap={3}>
              <HStack gap={3} vAlign="center" wrap="wrap">
                <Heading level={2}>Payment</Heading>
                {status && <Badge label={status.label} variant={status.variant} />}
              </HStack>
              {!payment ? (
                <Text color="secondary">
                  {order.financial_status === "paid" ? "Paid" : "Nothing to pay for this order."}
                </Text>
              ) : (
                <>
                  <Text>
                    {payment.method_name ?? "Payment"}: {formatMoney(payment.amount, digits)}
                  </Text>
                  {payment.instructions && payment.status === "pending" && (
                    <Text color="secondary">
                      <span className="shop-pre-line">{payment.instructions}</span>
                    </Text>
                  )}
                  {payment.status === "processing" && (
                    <Text color="secondary">
                      Your payment is on its way. We&apos;ll email you once it arrives.
                    </Text>
                  )}
                </>
              )}
            </VStack>
          </Card>
          {to && (
            <Card padding={5}>
              <VStack gap={1}>
                <Heading level={2}>Ships to</Heading>
                <Text>{[to.first_name, to.last_name].filter(Boolean).join(" ")}</Text>
                {[
                  to.company,
                  to.line1,
                  to.line2,
                  [to.postal_code, to.city].filter(Boolean).join(" "),
                  to.province,
                ]
                  .filter(Boolean)
                  .map((line) => (
                    <Text key={line} color="secondary">
                      {line}
                    </Text>
                  ))}
                <Text color="secondary">{country.name}</Text>
              </VStack>
            </Card>
          )}
          <AppElements elements={confirmationElements} props={{ orderId: order.id }} />
          {after}
        </VStack>
      </Grid>
    </>
  );
}
