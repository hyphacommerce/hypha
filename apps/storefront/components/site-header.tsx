"use client";

// The header on every page of a country, Astryx's TopNav: the store's name, its top categories (each a link; one with
// subcategories discloses them under it, `NavMenu`, one open at a time), then search, the currency, the account and the
// cart, which opens the cart drawer. On phones the categories move to a menu drawer (`MobileNav`) with the currency and
// the account, by CSS. Static pages render it; the cart's count comes from the cart provider once the page is here.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { Icon } from "@astryxdesign/core/Icon";
import { Popover } from "@astryxdesign/core/Popover";
import { HStack } from "@astryxdesign/core/Stack";
import { TopNav, TopNavHeading, TopNavItem } from "@astryxdesign/core/TopNav";
import {
  Bars3Icon,
  MagnifyingGlassIcon,
  ShoppingBagIcon,
  UserIcon,
} from "@heroicons/react/24/outline";
import { useState } from "react";

import { useCart } from "./cart/cart-provider";
import { CloseOnNavigate } from "./close-on-navigate";
import { type CountryOption, CurrencySwitcher } from "./currency-switcher";
import MobileNav from "./mobile-nav";
import { type NavCategory, NavMenu } from "./nav-menu";
import { SearchBox } from "./search-box";

export type { NavCategory };

/** The phones' menu, loaded the first time it opens. */

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
  const { count, open } = useCart();
  const [isSearching, setIsSearching] = useState(false);
  const [menu, setMenu] = useState(false);
  const [menuLoaded, setMenuLoaded] = useState(false);
  // The category whose subcategories are shown: one at a time.
  const [openCategory, setOpenCategory] = useState<string | null>(null);
  return (
    <>
      {/* A suggestion or "Search for…" goes to another page: the popover and the
          open submenu close. */}
      <CloseOnNavigate
        onNavigate={() => {
          setIsSearching(false);
          setOpenCategory(null);
        }}
      />
      <TopNav
        label="Store"
        heading={
          <HStack gap={1} vAlign="center">
            <div className="shop-nav-menu-button">
              <Button
                label="Menu"
                variant="ghost"
                isIconOnly
                icon={<Icon icon={Bars3Icon} />}
                clickAction={() => {
                  setMenuLoaded(true);
                  setMenu(true);
                }}
              />
              {menuLoaded && (
                <MobileNav
                  isOpen={menu}
                  onOpenChange={setMenu}
                  categories={props.categories}
                  searchHref={props.searchHref}
                  accountHref={props.accountHref}
                  country={props.country}
                  countries={props.countries}
                />
              )}
            </div>
            <TopNavHeading heading={props.storeName} headingHref={props.homeHref} />
          </HStack>
        }
        startContent={
          <div className="shop-nav-categories">
            {props.categories.map((c) => (
              <NavMenu
                key={c.id}
                category={c}
                isOpen={openCategory === c.id}
                onOpenChange={(show) =>
                  setOpenCategory((cur) => (show ? c.id : cur === c.id ? null : cur))
                }
              />
            ))}
          </div>
        }
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
            <div className="shop-nav-currency">
              <CurrencySwitcher current={props.country} countries={props.countries} />
            </div>
            <div className="shop-nav-account">
              <TopNavItem
                label="Account"
                href={props.accountHref}
                isIconOnly
                icon={<Icon icon={UserIcon} />}
              />
            </div>
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
