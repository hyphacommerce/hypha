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

import { lineImage } from "@/components/line-image";
import { SummaryLine, Totals } from "@/components/totals";
import { api, countryOf } from "@/lib/api";
import { formatMoney } from "@/lib/money";
import { link, orderTokenCookie } from "@/lib/site";

/** Per request: it reads the shopper's order with their token (design note 0024, §7). */
export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Order confirmed" };

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
  const placed = new Intl.DateTimeFormat("en-GB", { dateStyle: "long" }).format(
    new Date(order.placed_at),
  );
  return (
    <VStack gap={8} maxWidth={960}>
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
              <Text color="secondary">
                {order.financial_status === "paid" ? "Paid" : "Payment on delivery"}
              </Text>
              {order.shipping_lines.map((s) => (
                <Text key={s.group} color="secondary">
                  {s.name}
                  {s.estimate ? `, ${s.estimate.min_days}–${s.estimate.max_days} days` : ""}
                </Text>
              ))}
            </VStack>
          </Card>
          {to && (
            <Card padding={5}>
              <VStack gap={1}>
                <Heading level={2}>Ships to</Heading>
                <Text>{[to.first_name, to.last_name].filter(Boolean).join(" ")}</Text>
                {[to.company, to.line1, to.line2, [to.postal_code, to.city].filter(Boolean).join(" "), to.province]
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
