// Server-only: the translator of a request. Pages, layouts, route handlers and actions
// make it here; client components get theirs from the provider the country layout
// renders (`lib/use-t.ts`). Never import this from a "use client" file.
//
// Two languages, never mixed up:
// - The page's language is `pageLocale(segment)`'s (`lib/api.ts`, with the country),
//   read from the URL's locale segment: one of the country's own. A call to the API that
//   names a language takes this one.
// - A translator's `language` (`t.language`, `useLanguage()`) is the catalog's: what the
//   words are written in, `sr-Latn` for a page in `sr`, `en-XA` under the
//   pseudo-language. It's for formats and `<html lang>`, and is never sent to the API,
//   which would refuse or misread it.
//
// A language without a catalog (`de` today) keeps its own name: the translator's
// `language` is `de`, its words English (the fallback), its formats German, and
// `<html lang>` says `de`. The page's content (titles, descriptions, categories) is
// German and its URL and `hreflang` say so; the English words around it are a gap in the
// storefront's catalogs, closed by adding one, not a reason to call the page English.

import { type Messages, createTranslator, localeOf, pickNamespaces } from "@hyphacommerce/i18n";
import { cache } from "react";

import { loadStore, pageLocale } from "./api";
import { type Key, type Namespace, type T, PSEUDO, en, loadMessages, writtenIn } from "./messages";

/**
 * The translator for `language`, formatting for `country` (`de` in `CH` formats as
 * `de-CH`): one per request, so each message is compiled once. Its `locale` is what
 * every money, number and date format takes. A message that can't be formatted (a value
 * it needs wasn't given) shows its key and is logged: it mustn't take the page down.
 */
export const getT = cache(async (language: string, country?: string): Promise<T> => {
  const written = writtenIn(language);
  return createTranslator<Key>(written, await loadMessages(written), {
    fallback: en,
    locale: localeOf(written, country),
    onError: (error, key) => console.error(`i18n: ${key} (${written})`, error),
  });
});

/** The language a page in `language` is shown in: itself, or in development with
 * `HYPHA_I18N_PSEUDO=1` the pseudo-language (text still in plain English didn't come
 * from a catalog). Never sent to the API. */
function shown(language: string): string {
  const pseudo = process.env.NODE_ENV === "development" && process.env.HYPHA_I18N_PSEUDO === "1";
  return pseudo ? PSEUDO : language;
}

/** A page's translator, from the URL's locale segment (`params.country`): it reads
 * nothing of the request, so a static page stays static. */
export async function pageT(segment: string): Promise<T> {
  const { language, country } = await pageLocale(segment);
  return getT(shown(language), country);
}

/** An action's or a route handler's translator: its form sends the page's locale
 * segment (`useLocale`), so the answer is in the page's language. */
export async function actionT(segment: string): Promise<T> {
  return pageT(segment);
}

/** The translator where there's no place (a segment naming a country nobody sells to):
 * in the store's own language. */
export async function storeT(): Promise<T> {
  return getT(shown((await loadStore()).default_language));
}

/** The messages of `namespaces` for the browser, in `t`'s language: one catalog, with
 * English where the language lacks a message. */
export async function clientMessages(t: T, namespaces: readonly Namespace[]): Promise<Messages> {
  return pickNamespaces({ ...en, ...(await loadMessages(t.language)) }, namespaces);
}
