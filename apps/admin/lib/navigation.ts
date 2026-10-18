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
  /** Pages of the section, shown under it while you're in it. */
  children?: NavItem[];
};

/** The side nav's main items, in order. Settings comes last, on its own. */
export const MAIN_NAV: NavItem[] = [
  { label: "Home", href: "/", icon: HomeIcon },
  { label: "Orders", href: "/orders", icon: ShoppingBagIcon },
  {
    label: "Products",
    href: "/products",
    icon: CubeIcon,
    children: [{ label: "Categories", href: "/products/categories" }],
  },
  { label: "Customers", href: "/customers", icon: UsersIcon },
  { label: "Media", href: "/media", icon: PhotoIcon },
  {
    label: "Markets",
    href: "/markets",
    icon: GlobeEuropeAfricaIcon,
    children: [{ label: "Price lists", href: "/markets/price-lists" }],
  },
  { label: "Apps", href: "/apps", icon: PuzzlePieceIcon },
];

export const SETTINGS_NAV: NavItem = { label: "Settings", href: "/settings", icon: Cog6ToothIcon };

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
        description: "Name, address, language, time zone and the setup guide.",
      },
      {
        label: "Channels",
        href: "/settings/channels",
        icon: Squares2X2Icon,
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
        description: "The payment methods offered at checkout.",
      },
      {
        label: "Shipping",
        href: "/settings/shipping",
        icon: TruckIcon,
        description: "Zones, rates and live-rate apps.",
      },
      {
        label: "Search",
        href: "/settings/search",
        icon: MagnifyingGlassIcon,
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
        description: "Secret keys for your own code, publishable keys for storefronts.",
      },
      {
        label: "Webhooks and custom apps",
        href: "/settings/apps",
        icon: CodeBracketIcon,
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

/** The way back to the settings index, for a settings page's breadcrumb. */
export const SETTINGS_CRUMB = { label: SETTINGS_NAV.label, href: SETTINGS_NAV.href };

/** Whether `href` covers `pathname`: itself, or a page under it. */
export function covers(pathname: string, href: string): boolean {
  return href === "/" ? pathname === "/" : pathname === href || pathname.startsWith(`${href}/`);
}
