"use client";

// The header on every page of a country (design note 0023, §3): the store's name,
// search, the country picker and the cart, then the top categories.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { Icon } from "@astryxdesign/core/Icon";
import { Link } from "@astryxdesign/core/Link";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { ShoppingBagIcon } from "@heroicons/react/24/outline";

import { CountryPicker } from "./country-picker";
import { SearchBox } from "./search-box";

export type HeaderProps = {
  storeName: string;
  country: string;
  homeHref: string;
  searchHref: string;
  cartHref: string;
  cartCount: number;
  countries: { code: string; name: string; currency: string }[];
  categories: { id: string; name: string; href: string }[];
};

export function SiteHeader(props: HeaderProps) {
  const cart = props.cartCount > 0 ? `Cart, ${props.cartCount} items` : "Cart";
  return (
    <header className="shop-header">
      <div className="shop-page">
        <VStack gap={2} paddingBlock={3}>
          <HStack gap={4} vAlign="center" wrap="wrap">
            <Link href={props.homeHref} color="primary">
              <Text type="large" weight="semibold" as="span">
                <span style={{ fontFamily: "var(--font-family-heading)", fontSize: "1.35rem" }}>
                  {props.storeName}
                </span>
              </Text>
            </Link>
            <StackItem size="fill">
              <div style={{ minWidth: 200, maxWidth: 480 }}>
                <SearchBox country={props.country} searchHref={props.searchHref} />
              </div>
            </StackItem>
            <HStack gap={1} vAlign="center">
              <CountryPicker current={props.country} countries={props.countries} />
              <Button
                label={cart}
                href={props.cartHref}
                variant="ghost"
                icon={<Icon icon={ShoppingBagIcon} />}
                endContent={
                  props.cartCount > 0 ? <Badge label={String(props.cartCount)} /> : undefined
                }
              >
                Cart
              </Button>
            </HStack>
          </HStack>
          {props.categories.length > 0 && (
            <nav aria-label="Categories">
              <HStack gap={4} wrap="wrap">
                {props.categories.map((c) => (
                  <Link key={c.id} href={c.href} color="secondary">
                    {c.name}
                  </Link>
                ))}
              </HStack>
            </nav>
          )}
        </VStack>
      </div>
    </header>
  );
}
