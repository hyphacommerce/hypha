// The storefront's own strings: `messages/<language>/<namespace>.json`, flat ICU
// MessageFormat. English is the source: its keys type every translator, so a key that
// isn't there doesn't compile. Safe on both sides: catalogs are plain data.

import {
  type KeysOf,
  type Messages,
  type Translator,
  matchLanguage,
  mergeCatalogs,
  pseudo,
} from "@hyphacommerce/i18n";

import account from "../messages/en/account.json" with { type: "json" };
import address from "../messages/en/address.json" with { type: "json" };
import cart from "../messages/en/cart.json" with { type: "json" };
import checkout from "../messages/en/checkout.json" with { type: "json" };
import common from "../messages/en/common.json" with { type: "json" };
import discounts from "../messages/en/discounts.json" with { type: "json" };
import errors from "../messages/en/errors.json" with { type: "json" };
import footer from "../messages/en/footer.json" with { type: "json" };
import header from "../messages/en/header.json" with { type: "json" };
import home from "../messages/en/home.json" with { type: "json" };
import listing from "../messages/en/listing.json" with { type: "json" };
import order from "../messages/en/order.json" with { type: "json" };
import product from "../messages/en/product.json" with { type: "json" };
import sale from "../messages/en/sale.json" with { type: "json" };
import search from "../messages/en/search.json" with { type: "json" };

import { PSEUDO } from "./pseudo.ts";

const namespaces = {
  account,
  address,
  cart,
  checkout,
  common,
  discounts,
  errors,
  footer,
  header,
  home,
  listing,
  order,
  product,
  sale,
  search,
};

export const en = mergeCatalogs(namespaces);

/** Every message: `cart.title`, `checkout.pay`, … */
export type Key = KeysOf<typeof en>;

/** A catalog file's name: `cart`, `checkout`, … */
export type Namespace = keyof typeof namespaces;

/** The keys of the namespaces `N`. */
export type KeyIn<N extends Namespace> = Extract<Key, `${N}.${string}`>;

/** A translator of every message: what server code has. */
export type T = Translator<Key>;

/**
 * A translator of the namespaces `N` at least: what a function or a component shared by
 * servers and browsers asks for, so a browser's translator, which has only the
 * namespaces its page was sent, can be passed where it's enough.
 */
export type TFor<N extends Namespace> = Translator<KeyIn<N>>;

/**
 * The namespaces every page's client components read (the header, the cart drawer,
 * product cards, listings, the product form), sent to the browser with every page. The
 * rest stay on the server unless a route sends them: checkout and the account do, for
 * their forms (`components/messages.tsx`).
 */
export const SHARED_NAMESPACES = [
  "common",
  "header",
  "cart",
  "discounts",
  "product",
  "listing",
] as const satisfies readonly Namespace[];

export type SharedNamespace = (typeof SHARED_NAMESPACES)[number];

export { PSEUDO };

/** The languages with a catalog, English apart. */
const TRANSLATED = ["sr-Latn"] as const;

/** The translated catalog `language` reads, if there's one: its own, or the nearest
 * (`sr-Latn-RS` and plain `sr` both read `sr-Latn`). */
function translated(language: string): string | undefined {
  return matchLanguage([language], TRANSLATED);
}

/**
 * The language a page in `language` is written and formatted in: the language itself,
 * unless its catalog is another's. A store in plain `sr` reads the `sr-Latn` catalog, so
 * its dates and names are Latin too: `sr` alone would format them in Cyrillic beside
 * Latin words.
 */
export function writtenIn(language: string): string {
  const catalog = translated(language);
  if (catalog === undefined) return language;
  return language.toLowerCase().startsWith(catalog.toLowerCase()) ? language : catalog;
}

/**
 * A language's catalog, loaded when a request asks for it: the language's own, or the
 * nearest one there is. English for a language without one; a translator falls back to
 * English key by key for what a catalog lacks.
 */
export async function loadMessages(language: string): Promise<Messages> {
  if (language === PSEUDO) return pseudo(en);
  switch (translated(language)) {
    case "sr-Latn":
      return (await import("../messages/sr-Latn.ts")).default;
    default:
      return en;
  }
}

/** The messages of the `wanted` namespaces in `messages`, with English for what it lacks: one
 * catalog, as the browser is sent it. */
export function pick(messages: Messages, wanted: readonly Namespace[]): Messages {
  const prefixes = wanted.map((namespace) => `${namespace}.`);
  const picked: Record<string, string> = {};
  for (const [key, message] of Object.entries<string>(en)) {
    if (prefixes.some((prefix) => key.startsWith(prefix))) picked[key] = messages[key] ?? message;
  }
  return picked;
}

/** Whether `key` is a message: for keys built from the API's values (a status). */
export function isKey(key: string): key is Key {
  return Object.hasOwn(en, key);
}
