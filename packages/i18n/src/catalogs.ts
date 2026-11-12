// Catalogs by namespace: an app keeps `messages/<language>/<namespace>.json`, each a flat
// object of ICU strings, so two branches adding strings touch different files (or
// different lines of one). Merged, a key is `<namespace>.<key>`.

import type { Messages } from "./translator.ts";

/** A language's catalogs by namespace, as imported: `{cart: cartJson, checkout: checkoutJson}`. */
export type Namespaces = Readonly<Record<string, Messages>>;

/** The merged catalog's type: every `<namespace>.<key>` of `N`. */
export type MessagesOf<N extends Namespaces> = {
  readonly [K in keyof N & string as `${K}.${keyof N[K] & string}`]: string;
};

/** A catalog's keys: `KeysOf<typeof en>` is what an app's translator is typed with. */
export type KeysOf<M> = keyof M & string;

/** One catalog out of a language's namespaces, each key prefixed with its namespace. */
export function mergeCatalogs<const N extends Namespaces>(namespaces: N): MessagesOf<N> {
  const merged: Record<string, string> = {};
  for (const [namespace, messages] of Object.entries(namespaces)) {
    for (const [key, message] of Object.entries(messages)) {
      merged[`${namespace}.${key}`] = message;
    }
  }
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- built key by key above: exactly the keys the mapped type names
  return merged as MessagesOf<N>;
}
