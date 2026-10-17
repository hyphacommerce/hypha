"use client";

// The application frame, from Astryx's `shell-nav` template: a top bar with the brand
// and the user's menu, and a side nav with the admin's sections. A section's pages nest
// under it and show while you're in it, as in Shopify (design note 0024, section 6).

import { AppShell } from "@astryxdesign/core/AppShell";
import { DropdownMenu, DropdownMenuItem } from "@astryxdesign/core/DropdownMenu";
import { SideNav, SideNavItem, SideNavSection } from "@astryxdesign/core/SideNav";
import { TopNav } from "@astryxdesign/core/TopNav";
import {
  ArrowRightStartOnRectangleIcon,
  BuildingStorefrontIcon,
  CubeIcon,
  GlobeEuropeAfricaIcon,
  HomeIcon,
  MagnifyingGlassIcon,
  PhotoIcon,
  PuzzlePieceIcon,
  RocketLaunchIcon,
  ShoppingBagIcon,
  Squares2X2Icon,
  TruckIcon,
  UserGroupIcon,
  UsersIcon,
} from "@heroicons/react/24/outline";
import { usePathname } from "next/navigation";
import { startTransition, type ReactNode } from "react";

import { Wordmark } from "@/components/wordmark";

import { logout } from "../login/actions";

type NavItem = {
  label: string;
  href: string;
  icon?: typeof HomeIcon;
  /** Pages of the section, shown under it while you're in it. */
  children?: NavItem[];
};

const NAV: { title: string; items: NavItem[] }[] = [
  {
    title: "Store",
    items: [
      { label: "Home", href: "/", icon: HomeIcon },
      { label: "Orders", href: "/orders", icon: ShoppingBagIcon },
      {
        label: "Products",
        href: "/products",
        icon: CubeIcon,
        children: [{ label: "Categories", href: "/products/categories" }],
      },
      { label: "Media", href: "/media", icon: PhotoIcon },
      { label: "Customers", href: "/customers", icon: UsersIcon },
      {
        label: "Markets",
        href: "/markets",
        icon: GlobeEuropeAfricaIcon,
        children: [{ label: "Price lists", href: "/markets/price-lists" }],
      },
    ],
  },
  {
    title: "Settings",
    items: [
      { label: "Setup guide", href: "/setup", icon: RocketLaunchIcon },
      { label: "Store", href: "/settings/store", icon: BuildingStorefrontIcon },
      { label: "Channels", href: "/settings/channels", icon: Squares2X2Icon },
      { label: "Shipping", href: "/settings/shipping", icon: TruckIcon },
      { label: "Search", href: "/settings/search", icon: MagnifyingGlassIcon },
      {
        label: "Apps",
        href: "/settings/apps",
        icon: PuzzlePieceIcon,
        children: [{ label: "API keys", href: "/settings/api-keys" }],
      },
      { label: "Staff", href: "/settings/staff", icon: UserGroupIcon },
    ],
  },
];

/** Whether `href` covers `pathname`: itself, or a page under it. */
function covers(pathname: string, href: string): boolean {
  return href === "/" ? pathname === "/" : pathname === href || pathname.startsWith(`${href}/`);
}

/** Every item's href, flattened, to find the one that covers a page most closely. */
const ALL_HREFS = NAV.flatMap((s) => s.items.flatMap((i) => [i.href, ...(i.children ?? []).map((c) => c.href)]));

/** The nav item a page belongs to: the longest href that covers it. */
function currentHref(pathname: string): string | undefined {
  return ALL_HREFS.filter((href) => covers(pathname, href)).sort((a, b) => b.length - a.length)[0];
}

/** The signed-in staff member's menu, streamed in by the layout. */
export function UserMenu({ name, email }: { name: string; email: string }) {
  return (
    <DropdownMenu button={{ label: name, variant: "ghost", size: "sm" }} menuWidth={240}>
      <DropdownMenuItem label={name} description={email} />
      <DropdownMenuItem
        label="Sign out"
        icon={ArrowRightStartOnRectangleIcon}
        onClick={() => startTransition(() => logout())}
      />
    </DropdownMenu>
  );
}

export function AdminShell({ userMenu, children }: { userMenu: ReactNode; children: ReactNode }) {
  const pathname = usePathname();
  const current = currentHref(pathname);

  return (
    <AppShell
      contentPadding={0}
      topNav={<TopNav label="Hypha admin" heading={<Wordmark />} endContent={userMenu} />}
      sideNav={
        <SideNav resizable={{ defaultWidth: 240, minWidth: 180, maxWidth: 360 }}>
          {NAV.map((section) => (
            <SideNavSection key={section.title} title={section.title}>
              {section.items.map((item) => {
                const inSection = [item, ...(item.children ?? [])].some((i) => i.href === current);
                return (
                  <SideNavItem
                    key={item.href}
                    label={item.label}
                    href={item.href}
                    icon={item.icon}
                    isSelected={current === item.href}
                  >
                    {inSection &&
                      item.children?.map((child) => (
                        <SideNavItem
                          key={child.href}
                          label={child.label}
                          href={child.href}
                          isSelected={current === child.href}
                        />
                      ))}
                  </SideNavItem>
                );
              })}
            </SideNavSection>
          ))}
        </SideNav>
      }
    >
      {children}
    </AppShell>
  );
}
