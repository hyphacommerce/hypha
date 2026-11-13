// The admin's own strings: `messages/<language>/<namespace>.json`, flat ICU
// MessageFormat (`messages/README.md`). English is the source: its keys type every
// translator, so a key that isn't there doesn't compile.
//
// Server code only, for its values: this module holds the whole English catalog. A client
// component imports types from here (`Key`, `TFor`…) and gets its messages from the
// provider (`useT()` in `lib/use-t.ts`), which loads the language's catalog as a chunk of
// its own (`lib/catalogs.ts`).

import type { KeysOf, Messages, Translator } from "@hyphacommerce/i18n";

import en, { type namespaces } from "../messages/en.generated";
import { LANGUAGES } from "../messages/languages.generated";
import { loadCatalog } from "./catalogs.ts";
import { PSEUDO } from "./pseudo.ts";

export { en, LANGUAGES, PSEUDO };

/** Every message: `common.save`, `nav.orders`, … */
export type Key = KeysOf<typeof en>;

/** A catalog file's name: `common`, `orders`, … */
export type Namespace = keyof typeof namespaces;

/** The keys of the namespaces `N`. */
export type KeyIn<N extends Namespace> = Extract<Key, `${N}.${string}`>;

/** A translator of every message: what server code has. */
export type T = Translator<Key>;

/**
 * A translator of the namespaces `N` at least: what a function or a component shared by
 * servers and browsers asks for (`cancelReasonLabel(t: TFor<"orders">, …)`), so a
 * client component's translator, typed with the namespaces it names, can be passed where
 * it's enough.
 */
export type TFor<N extends Namespace> = Translator<KeyIn<N>>;

/**
 * The namespaces any client component may read without naming them (`useT()`): the bare
 * vocabulary, the frame and the shared components. A component that reads its area's
 * names it: `useT<"orders">()`.
 */
export const SHARED_NAMESPACES = [
  "common",
  "components",
  "nav",
  "shell",
  "table",
] as const satisfies readonly Namespace[];

export type SharedNamespace = (typeof SHARED_NAMESPACES)[number];

/** A language's catalog, loaded when a request asks for it; English for one the admin lacks. */
export async function loadMessages(language: string): Promise<Messages> {
  return loadCatalog(language);
}

/** The messages of the `wanted` namespaces in `messages`, with English for what it lacks. */
export function pick(messages: Messages, wanted: readonly Namespace[]): Messages {
  const prefixes = wanted.map((namespace) => `${namespace}.`);
  const picked: Record<string, string> = {};
  for (const [key, message] of Object.entries<string>(en)) {
    if (prefixes.some((prefix) => key.startsWith(prefix))) picked[key] = messages[key] ?? message;
  }
  return picked;
}

/**
 * Whether `key` is a message: for keys built from the API's values (a status, an error
 * code). Server code only; where a translator is at hand, `hasKey(t, key)`
 * (`lib/keys.ts`) asks the same on both sides.
 */
export function isKey(key: string): key is Key {
  return Object.hasOwn(en, key);
}
