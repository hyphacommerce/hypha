import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";

import { CartLine } from "@/components/cart-line";
import { lineImage } from "@/components/line-image";
import { Totals } from "@/components/totals";
import { api, countryOf } from "@/lib/api";
import { formatMoney } from "@/lib/money";
import { cartId, link } from "@/lib/site";

export const metadata: Metadata = { title: "Cart" };

/**
 * The cart (design note 0023, §3): lines with quantities, unavailable ones saying why
 * (they're left out of the totals), then checkout.
 */
export default async function CartPage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const id = await cartId(code);
  const cart = id
    ? (await api().GET("/storefront/carts/{id}", { params: { path: { id } } })).data
    : undefined;
  const open = cart && !cart.order ? cart : undefined;
  const digits = country.minor_units;

  if (!open || open.lines.length === 0) {
    return (
      <EmptyState
        headingLevel={1}
        title="Your cart is empty"
        description="Find something you like and add it here."
        actions={<Button label="Keep shopping" href={link(code, "/")} variant="primary" />}
      />
    );
  }

  return (
    <VStack gap={6}>
      <Heading level={1} type="display-2">
        Cart
      </Heading>
      <div className="shop-split" data-variant="summary">
        <Card padding={5}>
          <VStack gap={0}>
            {open.lines.map((line, i) => (
              <VStack key={line.variant} gap={0}>
                {i > 0 && <Divider />}
                <CartLine
                  country={code}
                  variant={line.variant}
                  title={line.product_title ?? "A product no longer sold"}
                  image={lineImage(line.image)}
                  href={line.handle ? link(code, `/p/${line.handle}`) : null}
                  variantTitle={line.variant_title}
                  quantity={line.quantity}
                  total={line.status === "priced" ? formatMoney(line.total, digits) : undefined}
                  problem={line.status === "unavailable" ? line.reason : null}
                />
              </VStack>
            ))}
          </VStack>
        </Card>
        <div className="shop-sticky">
          <Card padding={5}>
            <VStack gap={4}>
              <Heading level={2}>Summary</Heading>
              <Totals
                subtotal={open.subtotal}
                shipping={open.shipping_total}
                tax={open.tax_total}
                total={open.total}
                taxesIncluded={open.taxes_included}
                digits={digits}
              />
              {open.fully_available ? (
                <Button
                  label="Check out"
                  href={link(code, "/checkout")}
                  variant="primary"
                  size="lg"
                  width="100%"
                />
              ) : (
                <Banner status="warning" title="Remove what can't be bought to check out." />
              )}
              <Button label="Keep shopping" href={link(code, "/")} variant="ghost" width="100%" />
            </VStack>
          </Card>
        </div>
      </div>
    </VStack>
  );
}
