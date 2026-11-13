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

interface Provided {
  translator: Translator;
  language: string;
  messages: Messages;
}

const I18nContext = createContext<Provided | null>(null);

/**
 * Gives the components under it their translator. A provider inside another of the same
 * language adds its messages to the outer one's (its own win), so a nested layout hands
 * over only the namespaces its routes add.
 */
export function I18nProvider(props: I18nProviderProps): ReactNode {
  const { language, fallback, locale, onMissing, children } = props;
  const outer = useContext(I18nContext);
  const inherited = outer?.language === language ? outer.messages : undefined;
  const own = props.messages;
  const provided = useMemo((): Provided => {
    const messages = inherited ? { ...inherited, ...own } : own;
    const translator = createTranslator(language, messages, {
      fallback,
      locale,
      onMissing,
      // A message that can't be formatted shows its key: it mustn't take the page down.
      onError: (error, key) => console.error(`i18n: ${key} (${language})`, error),
    });
    return { translator, language, messages };
  }, [language, inherited, own, fallback, locale, onMissing]);
  return createElement(I18nContext, { value: provided }, children);
}

function useTranslator(): Translator {
  const provided = useContext(I18nContext);
  if (!provided) throw new Error("useT and useLanguage need an <I18nProvider> above them");
  return provided.translator;
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
