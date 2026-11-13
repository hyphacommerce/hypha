// An order as its buyer sees it: the lines and totals (with the discounts it got
// under them, and what each took off a line under that line), when it was placed and
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
import { AppliedDiscounts } from "@/components/discounts";
import { lineImage } from "@/components/line-image";
import { Price } from "@/components/price";
import { SummaryLine, Totals } from "@/components/totals";
import { type Country } from "@/lib/api";
import { linePrice, readOrderDiscounts } from "@/lib/discounts";
import { countryName, formatDate } from "@/lib/format";
import { type T, isKey } from "@/lib/messages";
import { formatMoney } from "@/lib/money";
import { elementsFor } from "@/lib/app-elements";

export type PlacedOrder = StorefrontSchemas["PlacedOrderResponse"];
type PaymentStatus = StorefrontSchemas["PaymentStatusDto"];

/** A payment's status for the buyer (an unknown one shows as it is: the enum is open). */
function paymentStatus(
  t: T,
  status: PaymentStatus,
): {
  label: string;
  variant: "success" | "warning" | "error" | "neutral";
} {
  const key = `order.payment_${status}`;
  const label = isKey(key) ? t(key) : status;
  switch (status) {
    case "pending":
      return { label, variant: "warning" };
    case "paid":
      return { label, variant: "success" };
    case "failed":
      return { label, variant: "error" };
    default:
      return { label, variant: "neutral" };
  }
}

/** An order's status word in a list (`order.fulfillment_fulfilled`); one this build
 * doesn't know shows as the API names it, its underscores as spaces. */
export function statusLabel(t: T, kind: "fulfillment" | "financial", status: string): string {
  const key = `order.${kind}_${status}`;
  return isKey(key) ? t(key) : status.replace(/_/g, " ");
}

/** The order's details, as two columns; `after` goes under them (a button). */
export async function OrderDetails({
  t,
  order,
  country,
  after,
}: {
  t: T;
  order: PlacedOrder;
  country: Country;
  after?: React.ReactNode;
}) {
  const digits = country.minor_units;
  const discounts = readOrderDiscounts(order);
  const to = order.shipping_address;
  const payment = order.payment ?? null;
  // An order paid online was paid before it existed; one paid by hand shows its
  // instructions (bank transfer, cash on delivery).
  const confirmationElements = await elementsFor("order.confirmation");
  const status = payment ? paymentStatus(t, payment.status) : undefined;
  const placed = formatDate(order.placed_at, t.locale);
  return (
    <>
      <Grid columns={{ minWidth: 320, repeat: "fit" }} gap={6}>
        <Card padding={5}>
          <VStack gap={4}>
            <Heading level={2}>{t("order.your_order")}</Heading>
            {order.lines.map((line) => {
              const shown = linePrice(
                t,
                line,
                discounts.lines[line.id],
                discounts.applications,
                digits,
              );
              return (
                <SummaryLine
                  t={t}
                  key={line.id}
                  quantity={line.quantity}
                  title={line.product_title}
                  image={lineImage(line.image)}
                  variant={line.variant_title}
                  notes={shown.notes}
                  total={
                    <Price t={t} price={shown.price} compareAt={shown.compareAt} digits={digits} />
                  }
                />
              );
            })}
            <Divider />
            <Totals
              t={t}
              subtotal={order.subtotal}
              shipping={order.shipping_total}
              tax={order.tax_total}
              total={order.total}
              taxesIncluded={order.taxes_included}
              digits={digits}
              discounts={discounts}
            />
            <AppliedDiscounts t={t} applications={discounts.applications} digits={digits} />
          </VStack>
        </Card>
        <VStack gap={4}>
          <Card padding={5}>
            <VStack gap={2}>
              <Heading level={2}>{t("order.details")}</Heading>
              <Text color="secondary">{t("order.placed_on", { date: placed })}</Text>
              {order.shipping_lines.map((s) => (
                <Text key={s.group} color="secondary">
                  {s.estimate
                    ? t("order.shipping_estimate", {
                        name: s.name,
                        min: s.estimate.min_days,
                        max: s.estimate.max_days,
                      })
                    : s.name}
                </Text>
              ))}
            </VStack>
          </Card>
          <Card padding={5}>
            <VStack gap={3}>
              <HStack gap={3} vAlign="center" wrap="wrap">
                <Heading level={2}>{t("order.payment")}</Heading>
                {status && <Badge label={status.label} variant={status.variant} />}
              </HStack>
              {!payment ? (
                <Text color="secondary">
                  {order.financial_status === "paid" ? t("order.paid") : t("order.nothing_to_pay")}
                </Text>
              ) : (
                <>
                  <Text>
                    {t("order.payment_line", {
                      method: payment.method_name ?? t("order.payment"),
                      amount: formatMoney(payment.amount, digits, t.locale),
                    })}
                  </Text>
                  {payment.instructions && payment.status === "pending" && (
                    <Text color="secondary">
                      <span className="shop-pre-line">{payment.instructions}</span>
                    </Text>
                  )}
                  {payment.status === "processing" && (
                    <Text color="secondary">{t("order.processing_note")}</Text>
                  )}
                </>
              )}
            </VStack>
          </Card>
          {to && (
            <Card padding={5}>
              <VStack gap={1}>
                <Heading level={2}>{t("order.ships_to")}</Heading>
                <Text>{[to.first_name, to.last_name].filter(Boolean).join(" ")}</Text>
                {[
                  to.company,
                  to.line1,
                  to.line2,
                  [to.postal_code, to.city].filter(Boolean).join(" "),
                  to.province_name ?? to.province,
                ]
                  .filter(Boolean)
                  .map((line) => (
                    <Text key={line} color="secondary">
                      {line}
                    </Text>
                  ))}
                <Text color="secondary">
                  {countryName(country.country, country.name, t.locale)}
                </Text>
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
