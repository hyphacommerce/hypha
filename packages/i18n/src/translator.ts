// The translator: one language's catalog, formatted with ICU MessageFormat.

import { IntlMessageFormat } from "intl-messageformat";

import { withEnglish } from "./languages.ts";

/** A catalog: full keys (`cart.items_count`) to ICU MessageFormat strings. */
export type Messages = Readonly<Record<string, string>>;

/** A catalog that may lack some of `Keys`: another language's, still being translated. */
export type MessagesFor<Keys extends string> = { readonly [K in Keys]?: string };

/** What a placeholder takes. */
export type Value = string | number | bigint | boolean | Date | null | undefined;

export type Values = Readonly<Record<string, Value>>;

/**
 * What a tag (`<link>…</link>`) is given: what's inside it, already formatted, and a
 * `key` unique within the message, for frameworks whose lists want one (React).
 */
export type TagFunction<T> = (chunks: (string | T)[], key: number) => string | T;

export type RichValues<T> = Readonly<Record<string, Value | T | TagFunction<T>>>;

/** Why a key couldn't be shown as its catalog has it. */
export interface Missing {
  key: string;
  language: string;
  /** `fallback`: shown from the fallback catalog; `key`: no catalog has it, the key is shown. */
  shown: "fallback" | "key";
}

export interface TranslatorOptions<Keys extends string = string> {
  /** The catalog for keys `messages` lacks: the source language's. */
  fallback?: MessagesFor<Keys>;
  /**
   * What numbers, dates and plurals are formatted for, when it's more than the language:
   * `de-CH` for German in Switzerland. Defaults to `language`.
   */
  locale?: string;
  /** Told once per key that `messages` lacks. */
  onMissing?: (missing: Missing) => void;
  /**
   * Told when a message can't be formatted (broken syntax, a value it needs wasn't
   * given); the key is shown in its place. Throws the error when absent.
   */
  onError?: (error: unknown, key: string) => void;
}

export interface Translator<Keys extends string = string> {
  /** The message for `key`, formatted with `values`. */
  (key: Keys, values?: Values): string;
  /**
   * The same where values may be functions for the message's tags and objects of the
   * caller's (elements): the parts in order, usable as React children.
   */
  rich<T>(key: Keys, values?: RichValues<T>): (string | T)[];
  /** Whether `key` is a message of this translator's catalog or its fallback. */
  has(key: string): boolean;
  readonly language: string;
  readonly locale: string;
}

/**
 * A translator for `language` over `messages`. A key `messages` lacks is read from
 * `fallback`, and one neither has is shown as the key itself. Compiled messages are
 * kept for the translator's life: make one per request and language (or per catalog),
 * not one per call.
 *
 * `Keys` is inferred from the catalogs; name it (`createTranslator<Key>(…)`) when they
 * were loaded as plain `Messages`.
 */
export function createTranslator<Keys extends string = string>(
  language: string,
  messages: MessagesFor<Keys>,
  options: TranslatorOptions<Keys> = {},
): Translator<Keys> {
  const { fallback, onMissing, onError } = options;
  const locale = options.locale ?? language;
  // A runtime without the locale's data would use its own default, which differs
  // between a server and a browser: English data is everywhere, so both agree.
  const locales = withEnglish(locale);
  const catalog: Readonly<Record<string, string | undefined>> = messages;
  const fallbackCatalog: Readonly<Record<string, string | undefined>> = fallback ?? {};
  const compiled = new Map<string, IntlMessageFormat | null>();

  function find(key: string): IntlMessageFormat | null {
    const known = compiled.get(key);
    if (known !== undefined) return known;
    let source = own(catalog, key);
    if (source === undefined) {
      source = own(fallbackCatalog, key);
      onMissing?.({ key, language, shown: source === undefined ? "key" : "fallback" });
    }
    let format: IntlMessageFormat | null = null;
    if (source !== undefined) {
      try {
        format = new IntlMessageFormat(source, locales);
      } catch (error) {
        report(error, key);
      }
    }
    compiled.set(key, format);
    return format;
  }

  function report(error: unknown, key: string): void {
    if (!onError) throw error;
    onError(error, key);
  }

  function parts<T>(key: string, values: RichValues<T> | undefined): (string | T)[] {
    const format = find(key);
    if (!format) return [key];
    try {
      const formatted = format.format<T>(values ? keyed(values) : undefined);
      return Array.isArray(formatted) ? formatted : [formatted];
    } catch (error) {
      report(error, key);
      return [key];
    }
  }

  const t = (key: Keys, values?: Values): string => parts<never>(key, values).join("");
  const has = (key: string): boolean =>
    own(catalog, key) !== undefined || own(fallbackCatalog, key) !== undefined;
  return Object.assign(t, { rich: parts, has, language, locale });
}

/**
 * A catalog's own entry: never `toString` or another inherited property, and never an
 * empty message, which is a translation not written yet rather than one that says nothing.
 */
function own(catalog: Readonly<Record<string, string | undefined>>, key: string) {
  const message = Object.hasOwn(catalog, key) ? catalog[key] : undefined;
  return message === undefined || message.trim() === "" ? undefined : message;
}

/** The values with each tag function handed a key: its position among the message's tags. */
function keyed<T>(
  values: RichValues<T>,
): Record<string, Value | T | ((chunks: (string | T)[]) => string | T)> {
  let next = 0;
  const out: Record<string, Value | T | ((chunks: (string | T)[]) => string | T)> = {};
  for (const [name, value] of Object.entries(values)) {
    out[name] = isTagFunction(value) ? (chunks) => value(chunks, next++) : value;
  }
  return out;
}

function isTagFunction<T>(value: Value | T | TagFunction<T>): value is TagFunction<T> {
  return typeof value === "function";
}
