// @hyphacommerce/i18n: the apps' own strings. Catalogs of ICU MessageFormat by language
// and namespace, a translator typed by the source catalog's keys, language matching and
// the storefront's locale segment, and a pseudo-language for development. Web APIs only.
// `./react` has the provider and hooks for client components, `./check` the catalog check.

export {
  type Messages,
  type MessagesFor,
  type Missing,
  type RichValues,
  type TagFunction,
  type Translator,
  type TranslatorOptions,
  type Value,
  type Values,
  createTranslator,
} from "./translator.ts";
export {
  type KeysOf,
  type MessagesOf,
  type NamespaceKeys,
  type Namespaces,
  mergeCatalogs,
  pickNamespaces,
} from "./catalogs.ts";
export {
  type Direction,
  type LocaleSegment,
  canonicalLanguage,
  directionOf,
  formatLocaleSegment,
  localeOf,
  matchLanguage,
  parseAcceptLanguage,
  parseLocaleSegment,
  withEnglish,
} from "./languages.ts";
export { pseudo, pseudoMessage } from "./pseudo.ts";
export { serbianLatin, toSerbianLatin } from "./serbian.ts";
