"use client";

// The page's place for client components: the URL's locale segment (`de`, `en-de`), which
// names the country and the language. Server actions are sent it (`useLocale()`), so
// their answers are in the page's language and their redirects stay in it; the server
// reads the country and the language from it again (`pageLocale`), never from the
// browser's word.

import { createContext, use, useMemo } from "react";

type Place = {
  /** The locale segment: what every action takes as `locale`. */
  segment: string;
  /** The home page here (`/en-de`; `/` in a single tree's default language). */
  home: string;
};

const LocaleContext = createContext<Place>({ segment: "", home: "/" });

export function LocaleProvider({ segment, home, children }: Place & { children: React.ReactNode }) {
  const value = useMemo(() => ({ segment, home }), [segment, home]);
  return <LocaleContext value={value}>{children}</LocaleContext>;
}

/** The page's locale segment, for server actions. */
export function useLocale(): string {
  return use(LocaleContext).segment;
}

/** The link to the home page in the page's country and language. */
export function useHome(): string {
  return use(LocaleContext).home;
}

/**
 * The page's path and query as the address bar has them, read when a picker is used: a
 * listing changes its query in place (`history.pushState`), so it's read then, not
 * rendered with.
 */
export function pagePath(): string {
  return `${window.location.pathname}${window.location.search}`;
}
