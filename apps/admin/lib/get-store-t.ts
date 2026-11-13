// Server-only: the translator for text that is saved as the store's content, in the
// store's default language.
//
//   const storeT = await getStoreT();
//   storeT("defaults.discount_free_shipping_title"); // "Free shipping"
//
// Everything a person reads in the admin comes from `getT()`, in their own language;
// this one is only for what a form is prefilled with and then stored. Client components
// get the resolved strings as props (`pickDefaults`, `lib/store-defaults.ts`).

import { LANGUAGES } from "../messages/languages.generated";
import { loadStore } from "./hypha";
import { getT } from "./i18n";
import {
  type PlainDefault,
  type StoreDefaults,
  type StoreT,
  pickDefaults,
  storeTranslator,
} from "./store-defaults";

/**
 * The translator for the store's default language (English when the admin has no catalog
 * for it, or when the store can't be read). It reads the store the layout already read,
 * so a page's shell may await it. Only behind the login.
 */
export async function getStoreT(): Promise<StoreT> {
  const store = await loadStore();
  return storeTranslator(store?.default_language, LANGUAGES, getT);
}

/** The defaults `names` in the store's default language, for a client component's prop. */
export async function getStoreDefaults<Name extends PlainDefault>(
  names: readonly Name[],
): Promise<StoreDefaults<Name>> {
  return pickDefaults(await getStoreT(), names);
}
