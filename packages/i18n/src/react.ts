"use client";
// @hyphacommerce/i18n/react: the translator for client components. A server layout
// renders `<I18nProvider language messages>` with the request's catalog (plain data, so
// it crosses to the browser); client components under it call `useT()`. Server
// components and actions don't use this: they call `createTranslator` themselves.

import { type ReactNode, createContext, createElement, useContext, useMemo } from "react";

import { type Messages, type Missing, type Translator, createTranslator } from "./translator.ts";

export interface I18nProviderProps {
  /** The language of `messages`. */
  language: string;
  /** The language's catalog. */
  messages: Messages;
  /** The catalog for keys `messages` lacks: the source language's. */
  fallback?: Messages;
  /** What numbers and dates are formatted for (`de-CH`). Defaults to `language`. */
  locale?: string;
  /** Told once per key `messages` lacks. Only a client component can pass it. */
  onMissing?: (missing: Missing) => void;
  children?: ReactNode;
}

const I18nContext = createContext<Translator | null>(null);

/** Gives the components under it their translator. */
export function I18nProvider(props: I18nProviderProps): ReactNode {
  const { language, messages, fallback, locale, onMissing, children } = props;
  const translator = useMemo(
    () =>
      createTranslator(language, messages, {
        fallback,
        locale,
        onMissing,
        // A message that can't be formatted shows its key: it mustn't take the page down.
        onError: (error, key) => console.error(`i18n: ${key} (${language})`, error),
      }),
    [language, messages, fallback, locale, onMissing],
  );
  return createElement(I18nContext, { value: translator }, children);
}

function useTranslator(): Translator {
  const translator = useContext(I18nContext);
  if (!translator) throw new Error("useT and useLanguage need an <I18nProvider> above them");
  return translator;
}

/**
 * The translator of the nearest `I18nProvider`. An app types it with its keys once
 * (`export const useAppT = () => useT<Key>()`), so an unknown key doesn't compile.
 */
export function useT<Keys extends string = string>(): Translator<Keys> {
  return useTranslator();
}

/** The language of the nearest `I18nProvider`. */
export function useLanguage(): string {
  return useTranslator().language;
}
