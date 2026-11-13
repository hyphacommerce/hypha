// Keys built from data (`errors.<code>`, a label by an API enum's value): checked against
// the translator's catalog before they're used, so an unknown value never shows a key.
// On both sides; server code with no translator at hand has `isKey` (`lib/messages.ts`).

import type { Translator } from "@hyphacommerce/i18n";

/** Whether `key` is a message `t` has (in its language or in English), and one of its keys. */
export function hasKey<Keys extends string>(t: Translator<Keys>, key: string): key is Keys {
  return t.has(key);
}
