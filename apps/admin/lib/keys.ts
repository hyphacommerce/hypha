// Keys built from data (`errors.<code>`, a label by an API enum's value): checked against
// the translator's catalog before they're used, so an unknown value never shows a key.
// On both sides; server code with no translator at hand has `isKey` (`lib/messages.ts`).

import type { Translator } from "@hyphacommerce/i18n";

/** Whether `key` is a message `t` has (in its language or in English), and one of its keys. */
export function hasKey<Keys extends string>(t: Translator<Keys>, key: string): key is Keys {
  return t.has(key);
}

/**
 * The message under a key built from data, or `fallback` when `t` has none: a label by
 * an API enum's value, with the value itself for one this build doesn't know. Where the
 * fallback is another message, write the choice out (`hasKey(t, key) ? t(key) : t(…)`),
 * so it's formatted only when it's shown.
 */
export function labelOr<Keys extends string, Fallback extends string | undefined>(
  t: Translator<Keys>,
  key: string,
  fallback: Fallback,
): string | Fallback {
  return hasKey(t, key) ? t(key) : fallback;
}
