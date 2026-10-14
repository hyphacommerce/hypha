"use client";

// The application frame, from Astryx's `shell-nav` template: a top bar with the brand
// and the user's menu, and a side nav with the admin's sections.

import { AppShell } from "@astryxdesign/core/AppShell";
import { DropdownMenu, DropdownMenuItem } from "@astryxdesign/core/DropdownMenu";
import { Icon } from "@astryxdesign/core/Icon";
import { HStack } from "@astryxdesign/core/Layout";
import { SideNav, SideNavItem, SideNavSection } from "@astryxdesign/core/SideNav";
import { Text } from "@astryxdesign/core/Text";
import { TopNav } from "@astryxdesign/core/TopNav";
import {
  ArrowRightStartOnRectangleIcon,
  BuildingStorefrontIcon,
  CubeIcon,
  GlobeEuropeAfricaIcon,
  HomeIcon,
  KeyIcon,
  ShoppingBagIcon,
  SparklesIcon,
  UsersIcon,
} from "@heroicons/react/24/outline";
import { usePathname } from "next/navigation";
import { startTransition, type ReactNode } from "react";

import { logout } from "../login/actions";

/** Sections not built yet are listed but disabled, so the shape of the admin shows. */
const NAV: {
  title: string;
  items: { label: string; href: string; icon: typeof HomeIcon; isReady: boolean }[];
}[] = [
  {
    title: "Store",
    items: [
      { label: "Home", href: "/", icon: HomeIcon, isReady: true },
      { label: "Orders", href: "/orders", icon: ShoppingBagIcon, isReady: false },
      { label: "Products", href: "/products", icon: CubeIcon, isReady: false },
      { label: "Customers", href: "/customers", icon: UsersIcon, isReady: false },
      { label: "Markets", href: "/markets", icon: GlobeEuropeAfricaIcon, isReady: false },
    ],
  },
  {
    title: "Settings",
    items: [
      { label: "Store", href: "/settings/store", icon: BuildingStorefrontIcon, isReady: false },
      { label: "Staff", href: "/settings/staff", icon: UsersIcon, isReady: false },
      { label: "API keys", href: "/settings/api-keys", icon: KeyIcon, isReady: false },
    ],
  },
];

export function AdminShell({
  userName,
  userEmail,
  children,
}: {
  userName: string;
  userEmail: string;
  children: ReactNode;
}) {
  const pathname = usePathname();

  return (
    <AppShell
      contentPadding={0}
      topNav={
        <TopNav
          label="Hypha admin"
          heading={
            <HStack gap={2} vAlign="center">
              <Icon icon={SparklesIcon} />
              <Text type="body" weight="bold">
                Hypha
              </Text>
            </HStack>
          }
          endContent={
            <DropdownMenu
              button={{ label: userName, variant: "ghost", size: "sm" }}
              menuWidth={240}
            >
              <DropdownMenuItem label={userName} description={userEmail} />
              <DropdownMenuItem
                label="Sign out"
                icon={ArrowRightStartOnRectangleIcon}
                onClick={() => startTransition(() => logout())}
              />
            </DropdownMenu>
          }
        />
      }
      sideNav={
        <SideNav resizable={{ defaultWidth: 240, minWidth: 180, maxWidth: 360 }}>
          {NAV.map((section) => (
            <SideNavSection key={section.title} title={section.title}>
              {section.items.map((item) => (
                <SideNavItem
                  key={item.href}
                  label={item.label}
                  href={item.isReady ? item.href : undefined}
                  icon={item.icon}
                  isSelected={pathname === item.href}
                  isDisabled={!item.isReady}
                />
              ))}
            </SideNavSection>
          ))}
        </SideNav>
      }
    >
      {children}
    </AppShell>
  );
}
