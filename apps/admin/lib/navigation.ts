// Where everything in the admin lives: the side nav, the settings index and the command
// palette all read these lists, so a page is added in one place.

import {
  ArchiveBoxIcon,
  BanknotesIcon,
  BuildingStorefrontIcon,
  ClipboardDocumentListIcon,
  CodeBracketIcon,
  Cog6ToothIcon,
  CpuChipIcon,
  CubeIcon,
  EnvelopeIcon,
  GlobeEuropeAfricaIcon,
  HomeIcon,
  KeyIcon,
  MagnifyingGlassIcon,
  PhotoIcon,
  PuzzlePieceIcon,
  ReceiptPercentIcon,
  ShieldCheckIcon,
  ShoppingBagIcon,
  Squares2X2Icon,
  TableCellsIcon,
  TruckIcon,
  UserCircleIcon,
  UserGroupIcon,
  UsersIcon,
} from "@heroicons/react/24/outline";

import type { Key, TFor } from "./messages";

export type NavIcon = typeof HomeIcon;

/** A message of the `nav` namespace: nav items hold keys, and whoever shows one calls `t`. */
export type NavKey = Extract<Key, `nav.${string}`>;

export type NavItem = {
  /** The item's name, as a message key: `t(item.label)`. */
  label: NavKey;
  href: string;
  icon?: NavIcon;
  /** The scope the page reads with; staff without it don't see the item. */
  scope?: string;
  /** Pages of the section, nested under it with a toggle. */
  children?: NavItem[];
};

/** A titled group of the side nav. */
export type NavSection = { title: NavKey; items: NavItem[] };

/**
 * Whether `scopes` (the signed-in staff member's, from `GET /admin/me`) include `scope`.
 * A write scope implies its read scope, as in the API.
 */
export function hasScope(scopes: readonly string[], scope: string): boolean {
  return (
    scopes.includes(scope) ||
    (scope.endsWith(":read") && scopes.includes(scope.replace(/:read$/, ":write")))
  );
}

/** Whether a page's item shows for a staff member with `scopes`. */
export function canOpen(item: { scope?: string }, scopes: readonly string[]): boolean {
  return item.scope === undefined || hasScope(scopes, item.scope);
}

/** `items` and their children, without the pages `scopes` can't open. */
export function visibleNav(items: NavItem[], scopes: readonly string[]): NavItem[] {
  return items
    .filter((item) => canOpen(item, scopes))
    .map((item) =>
      item.children
        ? { ...item, children: item.children.filter((child) => canOpen(child, scopes)) }
        : item,
    );
}

/** The side nav's sections, without the pages `scopes` can't open and no empty section. */
export function visibleSections(scopes: readonly string[]): NavSection[] {
  return NAV_SECTIONS.map((section) => ({
    ...section,
    items: visibleNav(section.items, scopes),
  })).filter((section) => section.items.length > 0);
}

export const SETTINGS_NAV: NavItem = {
  label: "nav.settings",
  href: "/settings",
  icon: Cog6ToothIcon,
};

/**
 * The side nav, in order. "Store" is what you sell and to whom; "Content" what
 * describes it beyond the catalog (files, your own fields; later menus and pages);
 * "Manage" how the admin itself is set up. The setup guide's item sits in "Manage",
 * above Settings, while the guide isn't finished.
 */
export const NAV_SECTIONS: NavSection[] = [
  {
    title: "nav.section_store",
    items: [
      { label: "nav.home", href: "/", icon: HomeIcon },
      { label: "nav.orders", href: "/orders", icon: ShoppingBagIcon, scope: "orders:read" },
      {
        label: "nav.products",
        href: "/products",
        icon: CubeIcon,
        scope: "products:read",
        children: [
          { label: "nav.categories", href: "/products/categories", scope: "products:read" },
        ],
      },
      { label: "nav.inventory", href: "/inventory", icon: ArchiveBoxIcon, scope: "inventory:read" },
      {
        label: "nav.purchasing",
        href: "/purchase-orders",
        icon: ClipboardDocumentListIcon,
        scope: "purchasing:read",
        children: [
          { label: "nav.purchase_orders", href: "/purchase-orders", scope: "purchasing:read" },
          { label: "nav.suppliers", href: "/suppliers", scope: "purchasing:read" },
          { label: "nav.transfers", href: "/transfers", scope: "inventory:read" },
        ],
      },
      {
        label: "nav.customers",
        href: "/customers",
        icon: UsersIcon,
        scope: "customers:read",
        children: [
          { label: "nav.customer_groups", href: "/customers/groups", scope: "customers:read" },
        ],
      },
      {
        label: "nav.markets",
        href: "/markets",
        icon: GlobeEuropeAfricaIcon,
        scope: "markets:read",
        children: [
          { label: "nav.price_lists", href: "/markets/price-lists", scope: "pricing:read" },
        ],
      },
      {
        label: "nav.discounts",
        href: "/discounts",
        icon: ReceiptPercentIcon,
        scope: "discounts:read",
      },
    ],
  },
  {
    title: "nav.section_content",
    items: [
      { label: "nav.media", href: "/media", icon: PhotoIcon, scope: "media:read" },
      {
        label: "nav.custom_fields",
        href: "/custom-fields",
        icon: TableCellsIcon,
        scope: "custom_fields:read",
      },
    ],
  },
  {
    title: "nav.section_manage",
    items: [
      { label: "nav.apps", href: "/apps", icon: PuzzlePieceIcon, scope: "apps:read" },
      {
        label: "nav.agents",
        href: "/agents",
        icon: CpuChipIcon,
        scope: "agents:read",
        children: [{ label: "nav.proposals", href: "/agents/proposals", scope: "agents:read" }],
      },
      SETTINGS_NAV,
    ],
  },
];

export type SettingsPage = NavItem & { icon: NavIcon; description: NavKey };

/** The settings index's groups, each a card of pages. */
export const SETTINGS_GROUPS: { title: NavKey; description: NavKey; pages: SettingsPage[] }[] = [
  {
    title: "nav.settings_group_store",
    description: "nav.settings_group_store_description",
    pages: [
      {
        label: "nav.settings_store",
        href: "/settings/store",
        icon: BuildingStorefrontIcon,
        scope: "stores:read",
        description: "nav.settings_store_description",
      },
      {
        label: "nav.settings_channels",
        href: "/settings/channels",
        icon: Squares2X2Icon,
        scope: "channels:read",
        description: "nav.settings_channels_description",
      },
    ],
  },
  {
    title: "nav.settings_group_selling",
    description: "nav.settings_group_selling_description",
    pages: [
      {
        label: "nav.settings_payments",
        href: "/settings/payments",
        icon: BanknotesIcon,
        scope: "payments:read",
        description: "nav.settings_payments_description",
      },
      {
        label: "nav.settings_shipping",
        href: "/settings/shipping",
        icon: TruckIcon,
        scope: "shipping:read",
        description: "nav.settings_shipping_description",
      },
      {
        label: "nav.settings_checkout",
        href: "/settings/checkout",
        icon: ShieldCheckIcon,
        scope: "checkout_rules:read",
        description: "nav.settings_checkout_description",
      },
      {
        label: "nav.settings_search",
        href: "/settings/search",
        icon: MagnifyingGlassIcon,
        scope: "products:read",
        description: "nav.settings_search_description",
      },
    ],
  },
  {
    title: "nav.settings_group_notifications",
    description: "nav.settings_group_notifications_description",
    pages: [
      {
        label: "nav.settings_emails",
        href: "/settings/notifications",
        icon: EnvelopeIcon,
        scope: "notifications:read",
        description: "nav.settings_emails_description",
      },
    ],
  },
  {
    title: "nav.settings_group_team",
    description: "nav.settings_group_team_description",
    pages: [
      {
        label: "nav.settings_staff",
        href: "/settings/staff",
        icon: UserGroupIcon,
        scope: "users:read",
        description: "nav.settings_staff_description",
      },
    ],
  },
  {
    title: "nav.settings_group_developers",
    description: "nav.settings_group_developers_description",
    pages: [
      {
        label: "nav.settings_api_keys",
        href: "/settings/api-keys",
        icon: KeyIcon,
        scope: "api_keys:read",
        description: "nav.settings_api_keys_description",
      },
      {
        label: "nav.settings_apps",
        href: "/settings/apps",
        icon: CodeBracketIcon,
        scope: "apps:read",
        description: "nav.settings_apps_description",
      },
    ],
  },
];

/** The signed-in staff member's own page, from the account menu. */
export const ACCOUNT_NAV: NavItem = {
  label: "nav.account",
  href: "/account",
  icon: UserCircleIcon,
};

/** The settings groups with only the pages `scopes` can open, and no empty group. */
export function visibleSettings(scopes: readonly string[]): typeof SETTINGS_GROUPS {
  return SETTINGS_GROUPS.map((group) => ({
    ...group,
    pages: group.pages.filter((page) => canOpen(page, scopes)),
  })).filter((group) => group.pages.length > 0);
}

/** The way back to the settings index, for a settings page's breadcrumb. */
export function settingsCrumb(t: TFor<"nav">): { label: string; href: string } {
  return { label: t(SETTINGS_NAV.label), href: SETTINGS_NAV.href };
}

/** The way back to the list of apps, for a breadcrumb. */
export function appsCrumb(t: TFor<"nav">): { label: string; href: string } {
  return { label: t("nav.apps"), href: "/apps" };
}

/** The way back to the list of agents, for a breadcrumb. */
export function agentsCrumb(t: TFor<"nav">): { label: string; href: string } {
  return { label: t("nav.agents"), href: "/agents" };
}

/** Whether `href` covers `pathname`: itself, or a page under it. */
export function covers(pathname: string, href: string): boolean {
  return href === "/" ? pathname === "/" : pathname === href || pathname.startsWith(`${href}/`);
}
