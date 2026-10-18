import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Grid } from "@astryxdesign/core/Grid";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { Metadata } from "next";
import { cookies } from "next/headers";

import type { StorefrontSchemas } from "@hypha/sdk/storefront";

import { ReloadCart } from "@/components/cart/cart-provider";
import { lineImage } from "@/components/line-image";
import { SummaryLine, Totals } from "@/components/totals";
import { api, countryOf } from "@/lib/api";
import { formatMoney } from "@/lib/money";
import { link, orderTokenCookie } from "@/lib/site";

/** Per request: it reads the shopper's order with their token (design note 0024, §7). */
export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Order confirmed" };

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
      return { label: String(status), variant: "neutral" };
  }
}

type Props = {
  params: Promise<{ country: string; id: string }>;
  searchParams: Promise<{ token?: string | string[] }>;
};

/**
 * An order's confirmation (design note 0024, §7): read with the order's access token,
 * kept in an httpOnly cookie when it was placed here, or given in the link (`?token=`),
 * as a confirmation email would. Without the right token the order isn't shown: its ID
 * alone reads nothing.
 */
export default async function OrderConfirmedPage({ params, searchParams }: Props) {
  const { country: folder, id } = await params;
  const country = await countryOf(folder);
  const code = country.country;
  const digits = country.minor_units;
  const given = (await searchParams).token;
  const token =
    (typeof given === "string" ? given : undefined) ??
    (await cookies()).get(orderTokenCookie(id))?.value;
  const order = token
    ? (
        await api().GET("/storefront/orders/{id}", {
          params: { path: { id }, query: { token } },
        })
      ).data
    : undefined;

  if (!order) {
    return (
      <EmptyState
        headingLevel={1}
        title="We can't show this order"
        description="Open it from the link in your confirmation email, on the device you ordered on."
        actions={<Button label="Back to the shop" href={link(code, "/")} variant="primary" />}
      />
    );
  }

  const to = order.shipping_address;
  const payment = order.payment ?? null;
  const status = payment ? paymentStatus(payment.status) : undefined;
  const placed = new Intl.DateTimeFormat("en-GB", { dateStyle: "long" }).format(
    new Date(order.placed_at),
  );
  return (
    <VStack gap={8} maxWidth={960}>
      {/* The order was placed from the cart the header still counts: read it again. */}
      <ReloadCart />
      <VStack gap={2}>
        <HStack gap={3} vAlign="center" wrap="wrap">
          <Heading level={1} type="display-2">
            Thank you
          </Heading>
          <Badge label={`Order #${order.number}`} />
        </HStack>
        <Text type="large" color="secondary">
          Your order is placed. We'll write to {order.email} about it.
        </Text>
      </VStack>
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
                total={formatMoney(line.total, digits)}
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
          <div>
            <Button label="Keep shopping" href={link(code, "/")} variant="primary" />
          </div>
        </VStack>
      </Grid>
    </VStack>
  );
}
