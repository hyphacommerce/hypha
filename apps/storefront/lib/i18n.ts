// Server-only: the translator of a request. Pages, layouts, route handlers and actions
// make it here; client components get theirs from the provider the country layout
// renders (`lib/use-t.ts`). Never import this from a "use client" file.

import { type Messages, createTranslator, localeOf } from "@hyphacommerce/i18n";
import { cache } from "react";

import { loadStore } from "./api";
import { type Key, type Namespace, type T, en, loadMessages, pick } from "./messages";
import { pageLanguage, segmentCountry } from "./site";

/**
 * The translator for `language`, formatting for `country` (`de` in `CH` formats as
 * `de-CH`): one per request, so each message is compiled once. Its `locale` is what
 * every money, number and date format takes.
 */
export const getT = cache(async (language: string, country?: string): Promise<T> =>
  createTranslator<Key>(language, await loadMessages(language), {
    fallback: en,
    locale: localeOf(language, country),
  }),
);

/** A page's translator, from the URL's first segment (`params.country`): it reads
 * nothing of the request, so a static page stays static. */
export async function pageT(segment: string): Promise<T> {
  return getT(pageLanguage(segment, await loadStore()), segmentCountry(segment));
}

/** An action's or a route handler's translator, which knows the shopper's country but
 * not the page's URL. */
export async function actionT(country: string): Promise<T> {
  return getT(pageLanguage(undefined, await loadStore()), country);
}

/** The messages of `namespaces` for the browser, in `t`'s language: one catalog, with
 * English where the language lacks a message. */
export async function clientMessages(t: T, namespaces: readonly Namespace[]): Promise<Messages> {
  return pick(await loadMessages(t.language), namespaces);
}
