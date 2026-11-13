// Server-only: the translator for server components, route handlers and actions.
//
//   const t = await getT();
//   t("common.save");                      // "Save"
//   t("common.items_count", { count: 3 }); // "3 items"
//
// Client components call `useT()` (`lib/use-t.ts`) instead. Never import this from a
// "use client" file.

import { createTranslator } from "@hyphacommerce/i18n";

import { getLanguage } from "./language";
import { type Key, type T, en, loadMessages } from "./messages";
import { localeOf } from "./pick-language.ts";

export type { Key, T };

/**
 * One translator per language for the server's life, where the storefront makes one per
 * request: the admin's depends on the language alone (no country, no store), a catalog
 * never changes while the server runs, and a translator keeps each message it compiled,
 * so a request compiles nothing an earlier one already did.
 */
const translators = new Map<string, Promise<T>>();

/**
 * The translator for `language`, or for the request's own (`getLanguage()`) when none is
 * named. Its `locale` is what every number, money and date format takes. A key the
 * language lacks reads from English; a message that can't be formatted is logged and
 * shows its key, never an error page.
 */
export async function getT(language?: string): Promise<T> {
  const wanted = language ?? (await getLanguage());
  let translator = translators.get(wanted);
  if (!translator) {
    translator = loadMessages(wanted).then((messages) =>
      createTranslator<Key>(wanted, messages, {
        fallback: en,
        locale: localeOf(wanted),
        onError: (error, key) => console.error(`i18n: ${key} (${wanted})`, error),
      }),
    );
    translators.set(wanted, translator);
    // A chunk that failed to load may load next time.
    translator.catch(() => translators.delete(wanted));
  }
  return translator;
}
