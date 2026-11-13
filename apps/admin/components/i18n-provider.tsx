"use client";

// The admin's strings and Astryx's own, in the staff member's language, for every client
// component under the root layout (`useT()`, `useFormat()`, Astryx's components).
//
// The layout passes the language only. The catalogs are chunks this module loads for
// that language (`lib/catalogs.ts`, `lib/astryx-locales.ts`): the browser fetches them
// once and keeps them, where a catalog passed as a prop would travel in the page's
// payload on every navigation. The server's render waits for the same chunks, so the
// HTML already has the words and hydration finds them unchanged.

import { InternationalizationProvider } from "@astryxdesign/core/i18n";
import type { Messages } from "@hyphacommerce/i18n";
import { I18nProvider } from "@hyphacommerce/i18n/react";
import { type ReactNode, use } from "react";

import { loadAstryxCatalog } from "@/lib/astryx-locales";
import { loadCatalog } from "@/lib/catalogs";
import { formatLocale } from "@/lib/pick-language";

type Catalogs = {
  messages: Messages;
  /**
   * Astryx's catalog under the page's locale, as its provider takes it; none for English
   * or a language Astryx lacks. One object per language, so the provider's memo holds.
   */
  astryx: Record<string, Readonly<Record<string, string>>> | undefined;
};

/** Each language's catalogs, loading or loaded: one promise, so `use` reads it again for free. */
const catalogs = new Map<string, Promise<Catalogs>>();

function catalogsOf(language: string): Promise<Catalogs> {
  let loading = catalogs.get(language);
  if (!loading) {
    loading = Promise.all([
      loadCatalog(language),
      // Astryx falls back to English by itself: its catalog failing to load mustn't take
      // the page down.
      loadAstryxCatalog(language).catch(() => undefined),
    ]).then(([messages, astryx]) => ({
      messages,
      // Astryx looks a catalog up by its locale and that tag's shorter forms, so the
      // catalog sits under the locale it's given below.
      astryx: astryx && { [formatLocale(language)]: astryx },
    }));
    catalogs.set(language, loading);
    // A chunk that failed to load (a new deploy) may load after the reload.
    loading.catch(() => catalogs.delete(language));
  }
  return loading;
}

// The document already says which language it's in: start fetching its catalogs as soon
// as this module runs, before hydration asks for them.
if (typeof document !== "undefined" && document.documentElement.lang) {
  void catalogsOf(document.documentElement.lang).catch(() => undefined);
}

/** Gives the components under it their translator and Astryx's, both in `language`. */
export function I18n({ language, children }: { language: string; children: ReactNode }) {
  const { messages, astryx } = use(catalogsOf(language));
  // One locale for the page: the admin's formats and Astryx's (its calendar, its
  // numbers) write the same way.
  const locale = formatLocale(language);
  return (
    <I18nProvider language={language} locale={locale} messages={messages}>
      <InternationalizationProvider locale={locale} messages={astryx}>
        {children}
      </InternationalizationProvider>
    </I18nProvider>
  );
}
