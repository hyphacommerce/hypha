// Defaults that are saved as the store's content: the name a checkout rule preset starts
// with and the message shoppers read, a discount template's title, the names the setup
// wizard suggests. Content is written in the store's default language, never the staff
// member's: a French-speaking employee of a Serbian store prefills Serbian. Their words
// are the `defaults` namespace, read through a translator for the store's language
// (`getStoreT()` in `lib/get-store-t.ts`).
//
// Plain functions and types, for server and client code and `node --test`. Only server
// code has that translator: a client component that prefills gets the resolved strings
// as a prop (`StoreDefaults`), so the browser never loads a second language's catalog.

import { matchLanguage } from "@hyphacommerce/i18n";

import type { KeyIn, TFor } from "./messages";
import { DEFAULT_LANGUAGE } from "./pick-language.ts";

/** A translator that has the `defaults` namespace, in the store's default language. */
export type StoreT = TFor<"defaults">;

/** A default's name: its key without the namespace. */
export type DefaultName = KeyIn<"defaults"> extends `defaults.${infer Name}` ? Name : never;

/** The defaults that take no values, so they resolve to plain strings. */
export type PlainDefault = Exclude<DefaultName, "channel_key_name">;

/** Resolved defaults by name, as a server page hands them to a client component. */
export type StoreDefaults<Name extends PlainDefault = PlainDefault> = Readonly<
  Record<Name, string>
>;

/**
 * The catalog language a store's defaults are written in: the store's default language
 * when the admin has a catalog for it (or for a nearer form: `sr` reads `sr-Latn`), else
 * English. `available` are the languages with catalogs.
 */
export function storeLanguage(
  defaultLanguage: string | null | undefined,
  available: readonly string[],
): string {
  if (!defaultLanguage) return DEFAULT_LANGUAGE;
  return matchLanguage([defaultLanguage], available) ?? DEFAULT_LANGUAGE;
}

/**
 * The translator a store's defaults are read with: `translatorFor` the language
 * `storeLanguage` picks. The server passes `getT`.
 */
export function storeTranslator<Translator extends StoreT>(
  defaultLanguage: string | null | undefined,
  available: readonly string[],
  translatorFor: (language: string) => Promise<Translator>,
): Promise<Translator> {
  return translatorFor(storeLanguage(defaultLanguage, available));
}

/** The defaults `names`, resolved, for a client component's prop. */
export function pickDefaults<Name extends PlainDefault>(
  t: StoreT,
  names: readonly Name[],
): StoreDefaults<Name> {
  const picked: Partial<Record<Name, string>> = {};
  for (const name of names) {
    const key: KeyIn<"defaults"> = `defaults.${name}`;
    picked[name] = t(key);
  }
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- every name was just set
  return picked as StoreDefaults<Name>;
}

/** The names an option is commonly given, offered as quick fills. */
export const OPTION_NAME_DEFAULTS = [
  "option_name_size",
  "option_name_color",
  "option_name_material",
  "option_name_style",
] as const satisfies readonly PlainDefault[];

/** The names the manual payment presets start with. */
export const PAYMENT_NAME_DEFAULTS = [
  "payment_cash_on_delivery_name",
  "payment_bank_transfer_name",
] as const satisfies readonly PlainDefault[];

/** What the setup wizard prefills. */
export const WIZARD_DEFAULTS = [
  ...PAYMENT_NAME_DEFAULTS,
  "shipping_option_name",
  "storefront_key_name",
] as const satisfies readonly PlainDefault[];
