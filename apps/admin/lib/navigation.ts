// Where everything in the admin lives: the side nav, the settings index and the command
// palette all read these lists, so a page is added in one place.

import {
  BanknotesIcon,
  BuildingStorefrontIcon,
  CodeBracketIcon,
  Cog6ToothIcon,
  CubeIcon,
  EnvelopeIcon,
  GlobeEuropeAfricaIcon,
  HomeIcon,
  KeyIcon,
  MagnifyingGlassIcon,
  PhotoIcon,
  PuzzlePieceIcon,
  ShoppingBagIcon,
  Squares2X2Icon,
  TableCellsIcon,
  TruckIcon,
  UserCircleIcon,
  UserGroupIcon,
  UsersIcon,
} from "@heroicons/react/24/outline";

export type NavIcon = typeof HomeIcon;

export type NavItem = {
  label: string;
  href: string;
  icon?: NavIcon;
  /** The scope the page reads with; staff without it don't see the item. */
  scope?: string;
  /** Pages of the section, nested under it with a toggle. */
  children?: NavItem[];
};

/** A titled group of the side nav. */
export type NavSection = { title: string; items: NavItem[] };

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

export const SETTINGS_NAV: NavItem = { label: "Settings", href: "/settings", icon: Cog6ToothIcon };

/**
 * The side nav, in order. "Store" is what you sell and to whom; "Content" what
 * describes it beyond the catalog (files, your own fields; later menus and pages);
 * "Manage" how the admin itself is set up. The setup guide's item sits in "Manage",
 * above Settings, while the guide isn't finished.
 */
export const NAV_SECTIONS: NavSection[] = [
  {
    title: "Store",
    items: [
      { label: "Home", href: "/", icon: HomeIcon },
      { label: "Orders", href: "/orders", icon: ShoppingBagIcon, scope: "orders:read" },
      {
        label: "Products",
        href: "/products",
        icon: CubeIcon,
        scope: "products:read",
        children: [{ label: "Categories", href: "/products/categories", scope: "products:read" }],
      },
      {
        label: "Customers",
        href: "/customers",
        icon: UsersIcon,
        scope: "customers:read",
        children: [{ label: "Groups", href: "/customers/groups", scope: "customers:read" }],
      },
      {
        label: "Markets",
        href: "/markets",
        icon: GlobeEuropeAfricaIcon,
        scope: "markets:read",
        children: [{ label: "Price lists", href: "/markets/price-lists", scope: "pricing:read" }],
      },
    ],
  },
  {
    title: "Content",
    items: [
      { label: "Media", href: "/media", icon: PhotoIcon, scope: "media:read" },
      {
        label: "Custom fields",
        href: "/custom-fields",
        icon: TableCellsIcon,
        scope: "custom_fields:read",
      },
    ],
  },
  {
    title: "Manage",
    items: [
      { label: "Apps", href: "/apps", icon: PuzzlePieceIcon, scope: "apps:read" },
      SETTINGS_NAV,
    ],
  },
];

export type SettingsPage = NavItem & { icon: NavIcon; description: string };

/** The settings index's groups, each a card of pages. */
export const SETTINGS_GROUPS: { title: string; description: string; pages: SettingsPage[] }[] = [
  {
    title: "Store",
    description: "Who you are and where you sell.",
    pages: [
      {
        label: "Store details",
        href: "/settings/store",
        icon: BuildingStorefrontIcon,
        scope: "stores:read",
        description: "Name, address, language, time zone and the setup guide.",
      },
      {
        label: "Channels",
        href: "/settings/channels",
        icon: Squares2X2Icon,
        scope: "channels:read",
        description: "Your storefronts and other sales channels, and their keys.",
      },
    ],
  },
  {
    title: "Selling",
    description: "How shoppers pay, get their orders and find products.",
    pages: [
      {
        label: "Payments",
        href: "/settings/payments",
        icon: BanknotesIcon,
        scope: "payments:read",
        description: "The payment methods offered at checkout.",
      },
      {
        label: "Shipping",
        href: "/settings/shipping",
        icon: TruckIcon,
        scope: "shipping:read",
        description: "Zones, rates and live-rate apps.",
      },
      {
        label: "Search",
        href: "/settings/search",
        icon: MagnifyingGlassIcon,
        scope: "products:read",
        description: "Filters, synonyms and the search index.",
      },
    ],
  },
  {
    title: "Notifications",
    description: "What the store emails, and to whom.",
    pages: [
      {
        label: "Emails",
        href: "/settings/notifications",
        icon: EnvelopeIcon,
        scope: "notifications:read",
        description: "Templates, the sender and staff alerts.",
      },
    ],
  },
  {
    title: "Team",
    description: "Who can sign in, and what they may do.",
    pages: [
      {
        label: "Staff and roles",
        href: "/settings/staff",
        icon: UserGroupIcon,
        scope: "users:read",
        description: "Invite staff, change their roles, and define roles.",
      },
    ],
  },
  {
    title: "Developers",
    description: "Access for code: keys, webhooks and apps you connect by hand.",
    pages: [
      {
        label: "API keys",
        href: "/settings/api-keys",
        icon: KeyIcon,
        scope: "api_keys:read",
        description: "Secret keys for your own code, publishable keys for storefronts.",
      },
      {
        label: "Webhooks and custom apps",
        href: "/settings/apps",
        icon: CodeBracketIcon,
        scope: "apps:read",
        description: "Apps made by hand: their webhooks, events and signing secrets.",
      },
    ],
  },
];

/** The signed-in staff member's own page, from the account menu. */
export const ACCOUNT_NAV: NavItem = {
  label: "Your account",
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
export const SETTINGS_CRUMB = { label: SETTINGS_NAV.label, href: SETTINGS_NAV.href };

/** Whether `href` covers `pathname`: itself, or a page under it. */
export function covers(pathname: string, href: string): boolean {
  return href === "/" ? pathname === "/" : pathname === href || pathname.startsWith(`${href}/`);
}
