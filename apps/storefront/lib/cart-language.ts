// Which language a cart should be in when a page reads it. Pure: shared by the server
// code that reads carts and its tests.

import { canonicalLanguage } from "@hyphacommerce/i18n";

/** A tag as BCP 47 spells it, for comparing (`sr-latn` and `sr-Latn` are one). */
function spelled(tag: string): string {
  return (canonicalLanguage(tag) ?? tag).toLowerCase();
}

/**
 * The language to set on a cart read for a page in `page` (the page's API language,
 * `pageLocale`): the page's, when the cart reads in another, else `undefined`. What the
 * cart reads in is its answer's `Content-Language`, else the language it keeps; a cart
 * whose language neither says is left alone rather than written on every read.
 */
export function cartLanguageFor(
  cart: { language?: string | null },
  contentLanguage: string | null,
  page: string,
): string | undefined {
  const reads = contentLanguage?.trim() || cart.language;
  if (!reads) return undefined;
  return spelled(reads) === spelled(page) ? undefined : page;
}
