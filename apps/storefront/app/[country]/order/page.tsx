import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { Metadata } from "next";
import { cookies } from "next/headers";

import { SummaryLine, Totals } from "@/components/totals";
import { api, countryOf } from "@/lib/api";
import { formatMoney } from "@/lib/money";
import { link } from "@/lib/site";

/** Per request: it reads the shopper's cart or query (design note 0024, §7). */
export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Thank you" };

/**
 * The order's confirmation (design note 0023, §3), read through the checked-out cart:
 * it names its order, and its lines and totals are what was bought.
 */
export default async function OrderPage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const digits = country.minor_units;
  const id = (await cookies()).get(`order_${code.toLowerCase()}`)?.value;
  const cart = id
    ? (await api().GET("/storefront/carts/{id}", { params: { path: { id } } })).data
    : undefined;

  if (!cart?.order) {
    return (
      <EmptyState
        headingLevel={1}
        title="No recent order"
        actions={<Button label="Back to the shop" href={link(code, "/")} variant="primary" />}
      />
    );
  }

  const to = cart.shipping_address;
  return (
    <VStack gap={6} maxWidth={640}>
      <VStack gap={2}>
        <Heading level={1} type="display-2">
          Thank you
        </Heading>
        <Text type="large" color="secondary">
          Your order is placed. We'll write to {cart.email} about it.
        </Text>
      </VStack>
      <Card padding={5}>
        <VStack gap={4}>
          {cart.lines.map((line) => (
            <SummaryLine
              key={line.variant}
              quantity={line.quantity}
              title={line.product_title ?? "A product no longer sold"}
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
        </VStack>
      </Card>
      {to && (
        <Text color="secondary">
          Ships to {[to.first_name, to.last_name].filter(Boolean).join(" ")}, {to.line1}, {to.city}.
        </Text>
      )}
      <div>
        <Button label="Keep shopping" href={link(code, "/")} />
      </div>
    </VStack>
  );
}
