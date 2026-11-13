# The admin's strings

Every word the admin shows comes from a catalog here, read through
[`@hyphacommerce/i18n`](../../../packages/i18n/README.md). English is the source: it says
which keys exist, and a key that isn't in it doesn't compile.

```
messages/
  en/common.json          {"save": "Save", "items_count": "{count, plural, one {# item} other {# items}}"}
  en/orders.json          {"cancel_reason_fraud": "Fraud"}
  en.generated.ts         every en/<namespace>.json merged: keys are "<namespace>.<key>"
  languages.generated.ts  the languages, and each one's catalog as a chunk of its own
  astryx.generated.ts     Astryx's own catalog for each of those languages, likewise
```

The app-side files and their exports are named as in the storefront:

| File | What it has | Where it's used |
| --- | --- | --- |
| `lib/messages.ts` | `en`, `Key`, `Namespace`, `KeyIn<N>`, `T`, `TFor<N>`, `SHARED_NAMESPACES`, `loadMessages(language)`, `pick`, `isKey` | values on the server, types anywhere |
| `lib/i18n.ts` | `getT(language?)` | server components, actions, route handlers |
| `lib/use-t.ts` | `useT<N>()`, `useLanguage()` | client components |
| `lib/keys.ts` | `hasKey(t, key)` | anywhere |
| `lib/pseudo.ts` | `PSEUDO` (`"en-XA"`) | anywhere |
| `lib/get-format.ts` | `getFormat()` | server, behind the login |
| `lib/use-format.ts` | `useFormat()` | client components |
| `lib/format.ts` | `Format`, `createFormat(locale, {currencies, timeZone})` | anywhere |
| `lib/language.ts` | `getLanguage()`, `offeredLanguages()`, `rememberLanguage()` | server |

One thing differs from the storefront, on purpose. There a layout sends the browser the
namespaces its routes read, as a provider's prop. The admin's pages are all dynamic and
vinext renders the layout again on every navigation, so a prop would travel with each
one; here the root layout passes the language alone and the provider loads that
language's catalog as a chunk the browser caches (`components/i18n-provider.tsx`,
`lib/catalogs.ts`). The typed surface is the same: `useT()` knows the shared namespaces,
`useT<"orders">()` adds one.

## Extracting an area

1. **Put the area's English in its namespace file**, `messages/en/<namespace>.json`. The
   namespaces already exist (empty) for every area, so nothing else needs registering:
   `account`, `agents`, `api_keys`, `apps`, `categories`, `checkout`, `custom_fields`,
   `customers`, `discounts`, `home`, `inventory`, `markets`, `media`, `notifications`,
   `orders`, `payments`, `price_lists`, `products`, `purchasing`, `settings`, `setup`,
   `shipping`, `staff`, `suppliers`, `transfers`. Use only your area's files.
   A new namespace is a new file plus `pnpm --filter admin i18n:index`, which rewrites the
   two generated modules (committed; `i18n:check` fails while they're stale). No list is
   edited by hand.
2. **Replace the literals** with `t("<namespace>.<key>")` (below).
3. **Replace hard-coded formats** (`toLocaleString("en")`, `new Intl.…("en-GB")`,
   `formatMoney(x, currencies)`) with the `Format` helpers (below).
4. **Add the area's folders to `adminTranslated`** in the root `vite.config.ts`: one line
   per folder, sorted. From then on literal text in their JSX is a lint error.
5. `pnpm --filter admin i18n:check`, `pnpm fmt`, `pnpm lint`, `pnpm lint:react`,
   `pnpm typecheck`, `pnpm --filter admin test`.
6. Look at the area in the pseudo-language (below): anything still in plain letters
   didn't come from a catalog.

## Keys

- A file is a flat object. Keys are `snake_case`; the full key is `<namespace>.<key>`.
- Name a key for what it is, not for its words: `cancel_dialog_title`, not
  `cancel_order_question`. Prefix by screen or part where an area has many
  (`list_empty_title`, `detail_ships_to`, `receive_dialog_title`).
- Variants of one message share a stem: `rate_limited`, `rate_limited_seconds`.
- A label map over an API enum is one key per value with a shared prefix
  (`cancel_reason_customer`, `cancel_reason_fraud`), read with a template key:
  ``t(`orders.cancel_reason_${reason}`)`` compiles when `reason` is a union of the known
  values. For a value that may be unknown (the SDK types open enums as `string`), check
  first and show the value as it is otherwise: see `cancelReasonLabel` in `lib/format.ts`.
- Never build a key from free data without `hasKey(t, key)` (`lib/keys.ts`, anywhere) or
  `isKey(key)` (`lib/messages.ts`, server): they narrow the string to a key, and an
  unknown value is then shown as it is.
- One message per sentence. Don't glue translated fragments together: word order differs
  between languages. `"{count} of {total} shipped"`, not `t("x") + " of " + …`.
- Reuse `common.*` for the bare vocabulary (below). A sentence is its area's own, even
  when it reads like one elsewhere.
- English wording stays exactly as it is today.

## Writing messages (ICU MessageFormat)

```json
{
  "greeting": "Hello, {name}",
  "items_count": "{count, plural, one {# item} other {# items}}",
  "nothing_or_some": "{count, plural, =0 {No orders yet} one {# order} other {# orders}}",
  "status": "{status, select, active {Active} draft {Draft} other {Archived}}",
  "terms": "Read the <link>guide</link> or <b>ask the owner</b>.",
  "literal_braces": "An http(s) URL with '{order}' in it"
}
```

- **Plurals** are written in the message, never in code (`n === 1 ? … : …`). `#` is the
  number, formatted for the language. Put the whole phrase inside each branch.
- **Braces and angle brackets** that are meant literally are quoted with apostrophes:
  `'{order}'`, `'<'`. A plain apostrophe (`can't`) needs nothing.
- **Numbers** given as numbers are formatted for the language (`{count}` with 1234 shows
  "1,234"). Pass a string to show something as it is (an ID, a code).
- **Money and dates** are formatted first and passed in as strings:
  `t("orders.total_line", { total: format.money(order.total) })`.

### Sentences with links or bold: `t.rich`

```tsx
<Text>
  {t.rich("settings.staff_hint", {
    link: (chunks, key) => (
      <Link key={key} href="/settings/staff">
        {chunks}
      </Link>
    ),
    b: (chunks, key) => <strong key={key}>{chunks}</strong>,
  })}
</Text>
```

Each tag's function gets what's inside it and a `key` for its element. A value that is an
element can be passed as a plain placeholder too:
`t.rich("orders.placed_at", { time: <DateTime key="time" value={order.created_at} /> })`.

## Server and client

```tsx
// A server component, a route handler or a server action.
import { getT } from "@/lib/i18n";

export default async function Page() {
  const t = await getT(); // the request's language; getT("en") names one
  return <PageHeader title={t("orders.list_title")} />;
}
```

```tsx
"use client";
import { useT } from "@/lib/use-t";

export function CancelButton({ order }: { order: Order }) {
  const t = useT<"orders">(); // the shared namespaces, and orders.*
  return <Button label={t("common.cancel")} tooltip={t("orders.cancel_tooltip")} />;
}
```

- `getT()` isn't an API read: a page's shell may await it beside `params`. Its translator
  (`T`) has every message.
- `useT()` has the shared namespaces (`common`, `components`, `nav`, `shell`, `table`); a
  component names the others it reads: `useT<"orders" | "customers">()`. A key outside
  them doesn't compile.
- **A component shared by server and client components takes `t` as a prop** (a badge, a
  pager: `<PaymentBadge t={t} status={…} />`, `components/status.tsx`), typed
  `TFor<"components">`: "a translator with at least that namespace", which both a
  server's `T` and a client's `useT()` are. It has no `"use client"` and no hook, so it
  renders the same from either side.
- A plain function in `lib/` that builds text takes `t` the same way (and a `Format` when
  it formats): `cancelReasonLabel(t: TFor<"orders">, reason)`. A module-level constant
  can't hold translated text: hold keys (`label: "nav.orders"`, typed `KeyIn<"nav">`) and
  call `t` where it's shown, as `lib/navigation.ts` does, or build the list inside the
  component.
- **Never import a value from `lib/messages.ts`, `lib/i18n.ts` or a `*.generated.ts` in a
  client component**: that puts the whole English catalog in the page's JavaScript. Types
  are fine. Client components get messages from the provider only.
- A server action returns text already translated (`return { error: t("…") }`); the form
  shows it as it is.

### Errors from the API

`describe(error)` (`lib/hypha.ts`) is the sentence for an API error, by its `code`, in the
request's language. It's async:

```ts
if (error) return { error: await describe(error) };
```

Its sentences are `errors.<code>` in `messages/en/errors.json`: a new code the admin can
meet gets a line there. A code without one shows the API's English `error` to an English
reader, and `errors.unknown` with the API's sentence after it to anyone else. One code,
one sentence, under the code's own name; the few with a second form add a suffix
(`rate_limited_seconds`). `describeWith(t, error)` (`lib/errors.ts`) is the same with a
translator at hand, for a callback that can't await.

## Formats

Never name a locale (`"en"`, `"en-GB"`) and never call `toLocaleString`, `toFixed` for
display or `Intl.*` directly: ask the request's `Format`. It formats for the translator's
`locale` (`t.locale`: the language, with British English for `en`, as the admin always
wrote dates and money), and every `Intl` formatter under it is given
`withEnglish(locale)`, so a runtime without the locale's data writes English, the same
on the server and in the browser.

```ts
const format = await getFormat(); // server, behind the login
const format = useFormat();       // client
```

| Call | Answers |
| --- | --- |
| `format.number(value, options?)` | `"1,234.5"`; `options` as `Intl.NumberFormat` takes them |
| `format.percent(fraction, options?)` | `"12.5%"` from `0.125` |
| `format.money({amount, currency})` | `"€18.00"` from minor units |
| `format.unitCost({amount, currency})` | `"€4.125"` |
| `format.dateTime(rfc3339, "long" \| "short")` | `"15 Oct 2026, 09:30 CEST"` / `"15 Oct, 09:30"`, in the store's time zone |
| `format.date(rfc3339, "long" \| "short")` | a calendar date kept as noon UTC: `"11 Nov 2026"` / `"11 Nov"` |
| `format.list(items, "conjunction" \| "disjunction" \| "unit")` | `"A, B and C"` / `"A, B or C"` / `"A, B, C"` |
| `format.plural(count)` | `"one"`, `"few"`, `"other"`… (messages use ICU plurals instead) |
| `format.bytes(bytes)` | `"1.4 MB"` |
| `format.countryName(code)` | `"Croatia"` |
| `format.currencyName(code)` | `"Euro"`, or `undefined` |
| `format.languageName(tag)` | `"Serbian (Latin)"` |
| `format.compare(a, b)` | the language's sort order, for `toSorted` |

- In JSX, `<DateTime value short? />` and `<CalendarDate value short? />`
  (`components/date-time.tsx`) do the dates.
- `getFormat()` and `useFormat()` already hold the currency table and the store's time
  zone, so `formatMoney(x, currencies)` becomes `format.money(x)`. The old functions in
  `lib/format.ts` still take a `language` as their last argument and default to English;
  call sites should end up on `format.*`.
- Before the login there's no store: `createFormat(t.locale)` formats numbers and dates
  (UTC), not money.
- `lib/regions.ts`' `countryName`, `countryOptions` and `currencyOptions` take the locale
  last: pass `format.locale`.
- `Intl` used to compute (which calendar day a time falls on in a zone), not to show, is
  arithmetic: it keeps its fixed locale.
- The time zone is the store's, as before; the language only changes how a time is written.
- Parsing what someone typed (`parseMoney`) accepts `.` and `,` in every language.

## What isn't translated

- **Content from the API**: product titles, customer names, a discount's title, an app's
  name, a merchant's template. The admin shows content in the store's language whatever
  the staff member's is.
- **Codes and IDs**: SKUs, order numbers, handles, scope names (`orders:read`), event
  types, currency and country codes (their *names* come from `format.countryName` and
  friends), error codes.
- **Text the API wrote for developers**: an error's `error` string is shown only where
  `describe` says so.
- **Brand names** (`Hypha`, `Stripe`) and keyboard keys.
- **Marks that read the same everywhere**: `—`, `·`, `×`, `→`, `/`, `%`. The lint rule lets
  text without a letter through; anything else in JSX is a message. A name that must
  stay as it is goes in a constant (`const BRAND = "Hypha"`).
- `console` messages and thrown `Error`s meant for developers.

## The language

`getLanguage()` (`lib/language.ts`) decides it for every request: the staff member's
choice (the `hypha_admin_language` cookie, written by the profile page), else the first
language of the browser's `Accept-Language` the catalogs have (`sr` reads `sr-Latn`),
else English. The root layout sets
`<html lang dir>` from it and hands it to the providers; the login, invite and reset pages
use the same function. It has nothing to do with the store's content languages.

A second language is a folder: `messages/<tag>/<namespace>.json` for every namespace, then
`pnpm --filter admin i18n:index`. A language the admin ships must be complete: the
browser gets one catalog, its language's, as a chunk it keeps (never English beside it,
never through the page's payload), so `i18n:check` fails for a missing key. Astryx's own
strings follow by themselves: the index adds Astryx's shipped catalog for the language
(`messages/astryx.generated.ts`, `lib/astryx-locales.ts`; `sr-Latn` reads Astryx's
Cyrillic one through `serbianLatin`).

## The pseudo-language

`HYPHA_I18N_PSEUDO=1` in the environment of a development server
(`HYPHA_I18N_PSEUDO=1 pnpm admin:dev`; the Worker sees only what its config declares) offers
`en-XA` in **Your account → Language**: every catalog string comes out accented and about
a third longer (`Ýööüüŕ ááççööüüñţ`), Astryx's own in brackets. Text still in plain
letters wasn't extracted. It exists only in the admin (`PSEUDO` in `lib/pseudo.ts`): it's
never sent to the API.

## The checks

- `pnpm --filter admin i18n:check` (CI): the generated index is in step, every message is
  valid ICU, keys are snake_case, no language has a key English lacks or other
  placeholders than English's, every shipped language is complete.
- `i18n/no-literal-text` (in `pnpm lint`; the rule is `scripts/oxlint-i18n.mjs`, shared
  with the storefront) on the files listed in `adminTranslated`: no literal text between
  tags or in the props people read (`label`, `title`, `description`, `placeholder`, `alt`,
  `aria-label`). It doesn't know Astryx's other text props (`subtitle`, `tooltip`,
  `actionLabel`, `submitLabel`…) and sees JSX only, so those, and strings in objects and
  actions, are yours to find.
- TypeScript: `t("orders.titel")` doesn't compile.

## `common`: the shared vocabulary

Bare words and the smallest phrases, for buttons, column heads and field labels. Use them
instead of adding the same word to an area's file.

| Keys | |
| --- | --- |
| Actions | `save`, `save_changes`, `cancel`, `close`, `done`, `delete`, `remove`, `discard`, `edit`, `add`, `create`, `update`, `apply`, `confirm`, `continue`, `back`, `next`, `previous`, `skip`, `retry` ("Try again"), `reload`, `refresh`, `search`, `clear`, `reset`, `filter`, `sort`, `select`, `select_all`, `choose`, `change`, `view`, `open`, `show`, `hide`, `show_more`, `show_less`, `more`, `load_more`, `sign_in`, `sign_out`, `turn_on`, `turn_off`, `actions`, `copy`, `duplicate`, `download`, `upload`, `import`, `export`, `print`, `send`, `resend`, `enable`, `disable`, `archive`, `unarchive`, `learn_more` |
| Progress | `saving`, `saved`, `deleting`, `loading`, `copied` |
| States | `enabled`, `disabled`, `on`, `off`, `yes`, `no`, `none`, `all`, `any`, `other`, `optional`, `required`, `default`, `unknown`, `active`, `inactive`, `draft`, `archived`, `not_set`, `empty_value` ("—") |
| Fields | `name`, `title`, `description`, `status`, `type`, `code`, `key`, `id`, `handle`, `email`, `phone`, `password`, `address`, `country`, `countries`, `currency`, `language`, `time_zone`, `date`, `time`, `created`, `updated`, `note`, `notes`, `details`, `settings`, `summary` |
| Money and lines | `total`, `subtotal`, `quantity`, `price`, `amount`, `tax`, `shipping`, `discount`, `sku` |
| Things | `product(s)`, `variant(s)`, `category`/`categories`, `order(s)`, `customer(s)`, `market(s)`, `channel(s)`, `location(s)`, `supplier`, `app(s)`, `role`, `home`, `your_account` |
| Phrases | `something_went_wrong`, `try_again_later`, `reload_the_page`, `no_results`, `nothing_here_yet`, `cannot_be_undone`, `unsaved_changes`, `leave_without_saving`, `delete_named` (`{name}`), `items_count` (`{count}`), `selected_count` (`{count}`), `and_more` (`{count}`) |

Other shared namespaces: `nav` (the side nav, settings index, account menu), `shell` (the
command palette), `errors` (API error codes), `auth` (login, invite, reset, two-factor),
`table` (the resource table, pager, filters), `components` (pickers, dialogs, the save
bar, address fields, status badges, skeleton labels, app extensions), `global_error`.
Reuse a `components.*` or `table.*` string only from the component it belongs to.
