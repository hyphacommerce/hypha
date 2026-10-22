"use client";

// The application frame, from Astryx's `shell-nav` template: a top bar with the brand,
// the command palette and the user's menu, and a side nav in sections. A section's
// pages nest under it behind a toggle, open while you're in the section, so where
// there's more to find is visible from anywhere. Settings is one entry, leading to its
// own index page, so the nav stays short however many settings there are.

import { AppShell } from "@astryxdesign/core/AppShell";
import { DropdownMenu, DropdownMenuItem } from "@astryxdesign/core/DropdownMenu";
import { HStack } from "@astryxdesign/core/Layout";
import { SideNav, SideNavItem, SideNavSection } from "@astryxdesign/core/SideNav";
import { TopNav } from "@astryxdesign/core/TopNav";
import { ArrowRightStartOnRectangleIcon, RocketLaunchIcon } from "@heroicons/react/24/outline";
import { usePathname } from "next/navigation";
import { Fragment, type ReactNode, startTransition, useState } from "react";

import { Wordmark } from "@/components/wordmark";
import {
  ACCOUNT_NAV,
  NAV_SECTIONS,
  type NavItem,
  SETTINGS_NAV,
  covers,
  visibleSections,
} from "@/lib/navigation";

import { logout } from "../login/actions";

/** Every item's href, flattened, to find the one that covers a page most closely. */
const ALL_HREFS = NAV_SECTIONS.flatMap((section) => section.items).flatMap((item) => [
  item.href,
  ...(item.children ?? []).map((child) => child.href),
]);

/** The nav item a page belongs to: the longest href that covers it. */
function currentHref(pathname: string): string | undefined {
  const covering = ALL_HREFS.filter((href) => covers(pathname, href));
  return covering.toSorted((a, b) => b.length - a.length)[0];
}

/** The setup guide's nav item, while the guide isn't finished. */
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

/** The signed-in staff member's menu. */
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
  const inSection = [item, ...(item.children ?? [])].some((i) => i.href === current);
  // The pages under a section show while you're in it; the toggle overrides that until
  // you enter or leave a section, which opens or closes it again.
  const [toggled, setToggled] = useState<{ inSection: boolean; isOpen: boolean }>();
  const isOpen = toggled?.inSection === inSection ? toggled.isOpen : inSection;
  if (!item.children?.length) {
    return (
      <SideNavItem
        label={item.label}
        href={item.href}
        icon={item.icon}
        isSelected={current === item.href}
      />
    );
  }
  return (
    <SideNavItem
      label={item.label}
      href={item.href}
      icon={item.icon}
      isSelected={current === item.href}
      collapsible={{
        isCollapsed: !isOpen,
        onCollapsedChange: (isCollapsed) => setToggled({ inSection, isOpen: !isCollapsed }),
      }}
    >
      {item.children.map((child) => (
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
 * The nav's sections with the pages the staff member's `scopes` can open. `setupItem`
 * (the setup guide's, while it shows) goes above Settings.
 */
export function MainNav({
  scopes,
  setupItem,
}: {
  scopes: readonly string[];
  setupItem?: ReactNode;
}) {
  const current = currentHref(usePathname());
  return visibleSections(scopes).map((section) => (
    <SideNavSection key={section.title} title={section.title}>
      {section.items.map((item) => (
        <Fragment key={item.href}>
          {item.href === SETTINGS_NAV.href && setupItem}
          <NavEntry item={item} current={current} />
        </Fragment>
      ))}
    </SideNavSection>
  ));
}

export function AdminShell({
  nav,
  commandMenu,
  userMenu,
  children,
}: {
  /** The side nav's sections (`MainNav`), trimmed to the user's scopes. */
  nav: ReactNode;
  /** The ⌘K palette (`CommandMenu`), likewise. */
  commandMenu: ReactNode;
  userMenu: ReactNode;
  children: ReactNode;
}) {
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
        <SideNav resizable={{ defaultWidth: 240, minWidth: 180, maxWidth: 360 }}>{nav}</SideNav>
      }
    >
      {children}
    </AppShell>
  );
}
