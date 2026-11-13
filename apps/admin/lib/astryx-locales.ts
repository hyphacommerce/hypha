// Astryx's own strings (a pager's "Next page", a dialog's "Close") in the staff member's
// language: Astryx ships a catalog per locale, and the generated index
// (`messages/astryx.generated.ts`) has one lazy chunk for each language the admin has, so
// a browser downloads the one it shows. English is built into Astryx.

import { serbianLatin } from "@hyphacommerce/i18n";

import { ASTRYX_LOADERS } from "../messages/astryx.generated";

type AstryxCatalog = Readonly<Record<string, string>>;

/**
 * Astryx's catalog for `language`. `undefined` for English and for a language Astryx
 * lacks, whose components then speak English. Astryx's Serbian is Cyrillic only, so
 * `sr-Latn` reads it letter for letter in Latin.
 */
export async function loadAstryxCatalog(language: string): Promise<AstryxCatalog | undefined> {
  const load = Object.hasOwn(ASTRYX_LOADERS, language) ? ASTRYX_LOADERS[language] : undefined;
  if (!load) return undefined;
  const catalog = (await load()).default;
  return /^sr-Latn\b/i.test(language) ? serbianLatin(catalog) : catalog;
}
