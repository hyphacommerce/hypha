"use client";

// The application frame, from Astryx's `shell-nav` template: a top bar with the brand,
// the command palette and the user's menu, and a side nav with the admin's sections. A
// section's pages nest under it and show while you're in it, as in Shopify. Settings is
// one entry, leading to its own index page, so the nav stays short however many
// settings there are.

import { AppShell } from "@astryxdesign/core/AppShell";
import { DropdownMenu, DropdownMenuItem } from "@astryxdesign/core/DropdownMenu";
import { HStack } from "@astryxdesign/core/Layout";
import { SideNav, SideNavItem, SideNavSection } from "@astryxdesign/core/SideNav";
import { TopNav } from "@astryxdesign/core/TopNav";
import { ArrowRightStartOnRectangleIcon, RocketLaunchIcon } from "@heroicons/react/24/outline";
import { usePathname } from "next/navigation";
import { startTransition, type ReactNode } from "react";

import { Wordmark } from "@/components/wordmark";
import {
  ACCOUNT_NAV,
  MAIN_NAV,
  type NavItem,
  SETTINGS_NAV,
  covers,
  visibleNav,
} from "@/lib/navigation";

import { logout } from "../login/actions";

/** Every item's href, flattened, to find the one that covers a page most closely. */
const ALL_HREFS = [...MAIN_NAV, SETTINGS_NAV].flatMap((i) => [
  i.href,
  ...(i.children ?? []).map((c) => c.href),
]);

/** The nav item a page belongs to: the longest href that covers it. */
function currentHref(pathname: string): string | undefined {
  const covering = ALL_HREFS.filter((href) => covers(pathname, href));
  return covering.toSorted((a, b) => b.length - a.length)[0];
}

/** The setup guide's nav item; the layout streams it in while the guide isn't finished. */
export function SetupNavItem() {
  const pathname = usePathname();
  return (
    <SideNavItem
      label="Setup guide"
      href="/setup"
      icon={RocketLaunchIcon}
      isSelected={pathname === "/setup"}
    />
  );
}

/** The signed-in staff member's menu, streamed in by the layout. */
export function UserMenu({ name, email }: { name: string; email: string }) {
  return (
    <DropdownMenu button={{ label: name, variant: "ghost", size: "sm" }} menuWidth={240}>
      <DropdownMenuItem
        label={ACCOUNT_NAV.label}
        description={email}
        icon={ACCOUNT_NAV.icon}
        href={ACCOUNT_NAV.href}
      />
      <DropdownMenuItem
        label="Sign out"
        icon={ArrowRightStartOnRectangleIcon}
        onClick={() => startTransition(() => logout())}
      />
    </DropdownMenu>
  );
}

function NavEntry({ item, current }: { item: NavItem; current: string | undefined }) {
  // A section's pages show only while you're in it, so the nav stays short.
  const inSection = [item, ...(item.children ?? [])].some((i) => i.href === current);
  return (
    <SideNavItem
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
}

/**
 * The store's sections the staff member can open: all of them until their scopes are
 * known (`null`, while the layout streams them in), so the frame never waits.
 */
export function StoreNav({ scopes }: { scopes: readonly string[] | null }) {
  const current = currentHref(usePathname());
  return visibleNav(MAIN_NAV, scopes).map((item) => (
    <NavEntry key={item.href} item={item} current={current} />
  ));
}

export function AdminShell({
  storeNav,
  commandMenu,
  userMenu,
  setupItem,
  children,
}: {
  /** The store's sections (`StoreNav`), trimmed to the user's scopes once known. */
  storeNav: ReactNode;
  /** The ⌘K palette (`CommandMenu`), likewise. */
  commandMenu: ReactNode;
  userMenu: ReactNode;
  /** The setup guide's item, above Settings while the guide shows. */
  setupItem?: ReactNode;
  children: ReactNode;
}) {
  const current = currentHref(usePathname());

  return (
    <AppShell
      contentPadding={0}
      topNav={
        <TopNav
          label="Hypha admin"
          heading={<Wordmark />}
          endContent={
            <HStack gap={1} vAlign="center">
              {commandMenu}
              {userMenu}
            </HStack>
          }
        />
      }
      sideNav={
        <SideNav resizable={{ defaultWidth: 240, minWidth: 180, maxWidth: 360 }}>
          <SideNavSection title="Store">{storeNav}</SideNavSection>
          <SideNavSection title="Manage">
            {setupItem}
            <NavEntry item={SETTINGS_NAV} current={current} />
          </SideNavSection>
        </SideNav>
      }
    >
      {children}
    </AppShell>
  );
}
