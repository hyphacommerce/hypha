"use client";

// The phones' menu: a drawer from the start edge, as the cart's is from the end, with
// every category (its subcategories under it), the account and the currency, scrolling
// inside the drawer when it's taller than the screen. Mounted when it first opens
// (`SiteHeader`), and imported with the header rather than lazily: a lazy load revealed
// it through Suspense, which React animates with a view transition that snapshots the
// page, so the drawer flickered as it opened.

import { Button } from "@astryxdesign/core/Button";
import { Dialog } from "@astryxdesign/core/Dialog";
import { Divider } from "@astryxdesign/core/Divider";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

import { CloseOnNavigate } from "./close-on-navigate";
import { type CountryOption, CurrencySwitcher } from "./currency-switcher";
import type { NavCategory } from "./nav-menu";
import { SiteLink } from "./site-link";

export default function MobileNav({
  isOpen,
  onOpenChange,
  categories,
  searchHref,
  accountHref,
  country,
  countries,
}: {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  categories: NavCategory[];
  searchHref: string;
  accountHref: string;
  country: string;
  countries: CountryOption[];
}) {
  return (
    <>
      {/* Following a link closes it. */}
      <CloseOnNavigate onNavigate={() => onOpenChange(false)} />
      <Dialog
        isOpen={isOpen}
        onOpenChange={onOpenChange}
        position={{ top: 0, bottom: 0, start: 0 }}
        width={320}
        maxHeight="100dvh"
        style={{ height: "100dvh", borderRadius: 0 }}
        padding={0}
        aria-label="Menu"
      >
        <VStack gap={4} padding={5} className="shop-mobile-menu">
          <HStack hAlign="between" vAlign="center">
            <Heading level={2}>Menu</Heading>
            <Button
              label="Close"
              variant="ghost"
              size="sm"
              clickAction={() => onOpenChange(false)}
            />
          </HStack>
          <nav className="shop-mobile-nav" aria-label="Categories">
            {categories.map((c) => (
              <VStack key={c.id} gap={0}>
                <SiteLink href={c.href} className="shop-mobile-nav-link" data-level="1">
                  {c.name}
                </SiteLink>
                {c.children.map((child) => (
                  <SiteLink
                    key={child.id}
                    href={child.href}
                    className="shop-mobile-nav-link"
                    data-level="2"
                  >
                    {child.name}
                  </SiteLink>
                ))}
              </VStack>
            ))}
            <Divider />
            <SiteLink href={searchHref} className="shop-mobile-nav-link" data-level="1">
              All products
            </SiteLink>
            <SiteLink href={accountHref} className="shop-mobile-nav-link" data-level="1">
              Account
            </SiteLink>
          </nav>
          <Divider />
          <VStack gap={2}>
            <Text color="secondary">Currency</Text>
            <CurrencySwitcher current={country} countries={countries} layout="segments" />
          </VStack>
        </VStack>
      </Dialog>
    </>
  );
}
