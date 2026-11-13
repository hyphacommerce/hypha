# @hyphacommerce/i18n

Translations for Hypha's apps. Part of [Hypha](https://github.com/hyphacommerce/hypha), the open-source commerce
API in Rust. Hypha is in public beta: this package follows the API's version and may change
between 0.x releases.

Catalogs of [ICU MessageFormat](https://unicode-org.github.io/icu/userguide/format_parse/messages/)
strings (on `intl-messageformat`), a translator typed by the English catalog's keys, language
matching, the storefront's locale segment, a pseudo-language and a check for CI. Web APIs
only: it runs on Node, on Workers and in browsers, and has no framework in it besides the
optional React entry.

## Catalogs

An app keeps one folder per language and one file per namespace, each a flat object of
ICU strings. Two branches adding strings touch different files, or different lines:

```
messages/
  en/cart.json        {"title": "Your cart", "items_count": "{count, plural, one {# item} other {# items}}"}
  en/checkout.json    {"pay": "Pay {total}", "terms": "I accept the <link>terms</link>"}
  de/cart.json        {"title": "Ihr Warenkorb"}
```

English is the source: it says which keys exist. `mergeCatalogs` joins a language's
namespaces into one catalog whose keys are `<namespace>.<key>`, and its type carries them:

```ts
// lib/messages.ts
import { type KeysOf, type Messages, mergeCatalogs } from "@hyphacommerce/i18n";

import cart from "../messages/en/cart.json";
import checkout from "../messages/en/checkout.json";

export const en = mergeCatalogs({ cart, checkout });
/** "cart.title" | "cart.items_count" | "checkout.pay" | "checkout.terms" */
export type Key = KeysOf<typeof en>;

/** Another language's catalog, loaded when a request asks for it. */
export async function loadMessages(language: string): Promise<Messages> {
  if (language === "de") {
    const [cart] = await Promise.all([import("../messages/de/cart.json")]);
    return mergeCatalogs({ cart: cart.default });
  }
  return en;
}
```

## On the server

Server components, route handlers and actions make a translator themselves, with the
English catalog as its `fallback`. Wrapped in React's `cache`, the components of one
server render share a translator, so each message is compiled once per render. `cache`
only remembers during a server render: an action or a route handler gets a new
translator per call, which is fine (it compiles only the messages it reads).

```ts
// lib/i18n.ts
import { cache } from "react";
import { type Translator, createTranslator } from "@hyphacommerce/i18n";

import { type Key, en, loadMessages } from "./messages.ts";

export type T = Translator<Key>;

export const getT = cache(async (language: string): Promise<T> =>
  createTranslator<Key>(language, await loadMessages(language), { fallback: en }),
);
```

```tsx
const t = await getT(language);
t("cart.title"); // "Your cart"
t("cart.items_count", { count: 3 }); // "3 items"
t("cart.titel"); // doesn't compile
```

- A key the language lacks is read from `fallback`; a key no catalog has is shown as the
  key. `onMissing({key, language, shown})` is told once per key.
- Where a runtime has no data for the locale, plurals and numbers follow English
  (not the runtime's default), so a server and a browser show the same.
- Plurals, numbers and dates follow `language`, or `locale` when given
  (`createTranslator("de", messages, {locale: localeOf("de", "CH")})` formats for `de-CH`).
- A message that can't be formatted (broken syntax, a value it needs wasn't given)
  throws, unless `onError(error, key)` is given: then the key is shown.

### Tags

`t.rich` takes functions for a message's tags and answers the parts in order, usable as
React children. Each function gets a `key` for its element:

```tsx
<p>
  {t.rich("checkout.terms", {
    link: (chunks, key) => (
      <a key={key} href="/terms">
        {chunks}
      </a>
    ),
  })}
</p>
```

## In client components

A server layout hands the request's catalog to the provider (plain data, so it crosses
to the browser); client components under it call `useT()`. Merge the language over
English on the server and pass that one catalog: `messages` plus `fallback` would
serialise two catalogs into every page, where the merged one is the size of English.

```tsx
// app/layout.tsx (a server component)
import { I18nProvider } from "@hyphacommerce/i18n/react";

const messages = { ...en, ...(await loadMessages(language)) };

<I18nProvider language={language} messages={messages}>
  {children}
</I18nProvider>;
```

The provider's `fallback` prop is there for a client component that builds its own
catalogs; a server layout doesn't need it.

```tsx
// lib/use-t.ts
"use client";
import { useT as useAnyT } from "@hyphacommerce/i18n/react";
import type { Translator } from "@hyphacommerce/i18n";

import type { Key } from "./messages.ts";

export const useT = (): Translator<Key> => useAnyT<Key>();
```

```tsx
"use client";
import { useT } from "@/lib/use-t";

export function CartCount({ count }: { count: number }) {
  const t = useT();
  return <span>{t("cart.items_count", { count })}</span>;
}
```

`useLanguage()` answers the provider's language. Under the provider a message that
can't be formatted shows its key and is logged, never thrown.

## Languages

| Function | What it answers |
| --- | --- |
| `parseAcceptLanguage(header)` | the header's languages, most wanted first; no `*`, nothing with `q=0` |
| `matchLanguage(wanted, available)` | the wanted language the app has, in `available`'s spelling (below); `undefined` for none |
| `directionOf(language)` | `"rtl"` for Arabic, Hebrew, Persian, Urdu and the like (a script in the tag decides), else `"ltr"` |
| `localeOf(language, country?)` | the formatting locale: `de` + `CH` → `de-CH`; a tag with a region keeps it |
| `parseLocaleSegment(segment)` | a storefront URL's first segment: `de` → `{country: "de"}`, `en-de` → `{language: "en", country: "de"}`, `sr-latn-rs` → `{language: "sr-Latn", country: "rs"}`; `undefined` for anything else |
| `formatLocaleSegment({language?, country})` | its inverse, lower-cased |
| `canonicalLanguage(tag)` | the tag as BCP 47 spells it (`sr-latn` → `sr-Latn`), or `undefined` |

`matchLanguage` makes two passes over `wanted`. The first tries each tag exactly, then
with its last parts taken off (`de-CH` → `de`, `sr-Latn-RS` → `sr-Latn` → `sr`). Only
when that finds nothing for any wanted tag does the second widen: the first wanted tag
whose language an available tag shares gets the first such tag (`sr` or `sr-RS` →
`sr-Latn`, `de` → `de-AT`). So a match of the first kind for a later wanted tag beats a
widened one for an earlier tag. Widening never crosses scripts: a wanted `sr-Cyrl` or
`zh-Hant` only gets a tag with that script or with none, never `sr-Latn` or `zh-Hans`.

A tag sent to the API as a `language` must be one of the API's own (the store's or the
market's languages), picked with `matchLanguage(wanted, thoseLanguages)`: it answers in
the list's spelling. Never send `canonicalLanguage`'s output, `parseAcceptLanguage`'s or
`parseLocaleSegment`'s `language` as they are: they accept aliases and extensions
(`iw`, `de-u-co-phonebk`) that the API's `Language` refuses.

## The pseudo-language

`pseudo(messages)` accents every letter and doubles the vowels, leaving ICU syntax,
placeholders and tags as they are: in development, text still in plain English wasn't
taken from a catalog, and layouts meet strings about a third longer.

```ts
const messages = process.env.NODE_ENV === "development" && language === "en-XA" ? pseudo(en) : …;
// "Your cart" → "Ýööüüŕ çááŕţ"
```

`en-XA` is the app's own name for it, in development only: it's no language of any
store, so it must never be sent to the API (ask the API in English while showing it).

## The check

```
pnpm exec hypha-i18n-check messages [--source en] [--complete de]…
```

Fails (exit 1) when a message isn't ICU MessageFormat, when a language has a key English
lacks, when a message's placeholders or tags aren't English's for its key, or when a
language named with `--complete` lacks a key. What every other language still lacks is
printed and passes. It also reads `messages/<language>.json` files with full keys, but
a directory holds one layout or the other: both together are refused (exit 2). The
same check as a function: `checkCatalogs` from `@hyphacommerce/i18n/check`.

## License

MIT or Apache-2.0, at your option.
