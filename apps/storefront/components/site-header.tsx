"use client";

// The header on every page of a country (design note 0024, §7), Astryx's TopNav: the
// store's name, its top categories (a menu for one with subcategories), then search, the
// currency, the account and the cart, which opens the cart drawer. Static pages render
// it; the cart's count comes from the cart provider once the page is here.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { Icon } from "@astryxdesign/core/Icon";
import { Popover } from "@astryxdesign/core/Popover";
import { HStack } from "@astryxdesign/core/Stack";
import {
  TopNav,
  TopNavHeading,
  TopNavItem,
  TopNavMegaMenu,
  TopNavMegaMenuItem,
} from "@astryxdesign/core/TopNav";
import { MagnifyingGlassIcon, ShoppingBagIcon, UserIcon } from "@heroicons/react/24/outline";
import { usePathname } from "next/navigation";
import { useState } from "react";

import { useCart } from "./cart/cart-provider";
import { CloseOnNavigate } from "./close-on-navigate";
import { type CountryOption, CurrencySwitcher } from "./currency-switcher";
import { SearchBox } from "./search-box";

export type NavCategory = {
  id: string;
  name: string;
  href: string;
  children: { id: string; name: string; href: string }[];
};

export type HeaderProps = {
  storeName: string;
  country: string;
  homeHref: string;
  searchHref: string;
  accountHref: string;
  countries: CountryOption[];
  categories: NavCategory[];
};

export function SiteHeader(props: HeaderProps) {
  const pathname = usePathname();
  const { count, open } = useCart();
  const [isSearching, setIsSearching] = useState(false);
  return (
    <>
      {/* A suggestion or "Search for…" goes to another page: the popover closes. */}
      <CloseOnNavigate onNavigate={() => setIsSearching(false)} />
      <TopNav
        label="Store"
        heading={<TopNavHeading heading={props.storeName} headingHref={props.homeHref} />}
        startContent={props.categories.map((c) =>
          c.children.length > 0 ? (
            <TopNavMegaMenu
              // Keyed by the page too: a mega menu takes no open state, so a new page
              // remounts it closed.
              key={`${c.id}:${pathname}`}
              label={c.name}
              items={[c, ...c.children].map((item) => (
                <TopNavMegaMenuItem
                  key={item.id}
                  title={item.id === c.id ? `All ${c.name}` : item.name}
                  href={item.href}
                />
              ))}
            />
          ) : (
            <TopNavItem
              key={c.id}
              label={c.name}
              href={c.href}
              isSelected={pathname.startsWith(c.href)}
            />
          ),
        )}
        endContent={
          <HStack gap={1} vAlign="center">
            <Popover
              isOpen={isSearching}
              onOpenChange={setIsSearching}
              label="Search"
              placement="below"
              alignment="end"
              width={420}
              content={<SearchBox country={props.country} searchHref={props.searchHref} />}
            >
              <Button
                label="Search"
                variant="ghost"
                isIconOnly
                icon={<Icon icon={MagnifyingGlassIcon} />}
              />
            </Popover>
            <CurrencySwitcher current={props.country} countries={props.countries} />
            <TopNavItem
              label="Account"
              href={props.accountHref}
              isIconOnly
              icon={<Icon icon={UserIcon} />}
            />
            <Button
              label="Cart"
              aria-label={count > 0 ? `Cart, ${count} items` : "Cart"}
              variant="ghost"
              icon={<Icon icon={ShoppingBagIcon} />}
              endContent={count > 0 ? <Badge label={String(count)} /> : undefined}
              clickAction={open}
            />
          </HStack>
        }
      />
    </>
  );
}
