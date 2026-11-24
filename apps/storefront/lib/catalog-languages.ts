// Which languages have a catalog of their own, apart from the catalogs themselves, so
// code shared with browsers can ask without bundling every message.

import { matchLanguage } from "@hyphacommerce/i18n";

/** The languages with a catalog, English apart. */
const TRANSLATED = ["sr-Latn"] as const;

/** The translated catalog `language` reads, if there's one: its own, or the nearest
 * (`sr-Latn-RS` and plain `sr` both read `sr-Latn`). */
export function translated(language: string): string | undefined {
  return matchLanguage([language], TRANSLATED);
}
