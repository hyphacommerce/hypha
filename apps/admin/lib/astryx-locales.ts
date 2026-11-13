// Astryx's own strings (a pager's "Next page", a dialog's "Close") in the staff member's
// language: Astryx ships a catalog per locale, and the generated index
// (`messages/astryx.generated.ts`) has one lazy chunk for each language the admin has, so
// a browser downloads the one it shows. English is built into Astryx. Where Astryx's
// catalog lacks keys its English has (the rich text editor's, in 0.7's Serbian), the
// admin's supplement (`astryx-messages/<language>.json`) fills them; `i18n:check` lists
// what is still missing after an Astryx upgrade.

import { serbianLatin } from "@hyphacommerce/i18n";

import { ASTRYX_LOADERS, ASTRYX_SUPPLEMENTS } from "../messages/astryx.generated";

type AstryxCatalog = Readonly<Record<string, string>>;

/**
 * Astryx's catalog for `language` with the admin's supplement over it. `undefined` for
 * English and for a language neither has, whose components then speak English. Astryx's
 * Serbian is Cyrillic only, so `sr-Latn` reads it letter for letter in Latin; the
 * supplement is written in the language's own script and goes over that.
 */
export async function loadAstryxCatalog(language: string): Promise<AstryxCatalog | undefined> {
  const load = Object.hasOwn(ASTRYX_LOADERS, language) ? ASTRYX_LOADERS[language] : undefined;
  const loadSupplement = Object.hasOwn(ASTRYX_SUPPLEMENTS, language)
    ? ASTRYX_SUPPLEMENTS[language]
    : undefined;
  if (!load && !loadSupplement) return undefined;
  const [shipped, supplement] = await Promise.all([load?.(), loadSupplement?.()]);
  const catalog =
    shipped && /^sr-Latn\b/i.test(language) ? serbianLatin(shipped.default) : shipped?.default;
  return { ...catalog, ...supplement?.default };
}
