// A language's catalog as a chunk of its own, for the server and the browser alike. No
// catalog is imported here statically: the browser downloads the one language it shows,
// once, and its cache keeps it across navigations.

import { type Messages, pseudo } from "@hyphacommerce/i18n";

import { type AppLanguage, LANGUAGES, LOADERS } from "../messages/languages.generated";
import { DEFAULT_LANGUAGE } from "./pick-language.ts";
import { PSEUDO } from "./pseudo.ts";

/** Whether the admin has a catalog for `language`. */
export function isAppLanguage(language: string): language is AppLanguage {
  return LANGUAGES.some((known) => known === language);
}

/**
 * The catalog of `language`: every key, since a language the admin ships is complete
 * (`pnpm --filter admin i18n:check`), so a browser never needs English beside it. The
 * pseudo-language is English run through `pseudo`; a language the admin lacks is English.
 */
export async function loadCatalog(language: string): Promise<Messages> {
  if (language === PSEUDO) return pseudo(await LOADERS[DEFAULT_LANGUAGE]());
  return LOADERS[isAppLanguage(language) ? language : DEFAULT_LANGUAGE]();
}
