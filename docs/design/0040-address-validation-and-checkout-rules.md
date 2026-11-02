# 0040: Address validation, autocomplete and checkout rules

- **Status:** Accepted (2026-11-02), with §14's hard-to-reverse choices and §15's one new
  edge (checkout → checkout-rules). Danilo read the research (§1) and the
  recommendations AV1–AV10 on 2026-11-02 and decided three questions:
  - Autocomplete is in this note (AV11).
  - A PO box and a courier option meet as an error on the chosen option now; hiding
    options for an address comes later (AV12).
  - Setting an address keeps answering `422` for a format problem; a softer `200` with
    messages is recorded in §13 for later (AV3).
- **Scope:**
  - Checking an address against its country's format: required fields, postal code
    format, region names and codes, postal code against region, house number.
  - Verifying an address with a provider (through an app): deliverable or not, suggested
    corrections, flags such as PO box and residential.
  - Autocomplete while the shopper types, through an app.
  - Checkout rules a merchant sets up without code: conditions on the address and the
    cart, ending in an error or a warning with the merchant's message on a field.
  - An extension point for apps to validate a cart at checkout.
  - Messages on the cart (errors and warnings, each pointing at a field), and checkout
    refusing a cart with errors.
  - The address check kept on the order, "Mark as valid", and editing an order's shipping
    address.
- **Not in scope:**
  - Billing addresses (carts and orders have none yet).
  - Shipping zones and tax regions by region or postal code. AV5's region codes make
    them possible; they get their own note.
  - Hiding or renaming shipping options for an address (AV12, §13).
  - Translated messages and labels (the languages work, 0021 §5).
  - Functions (sandboxed wasm, 0029 §11).
  - Verifying the addresses in customers' address books.
  - These are §13's list.
- **Related:**
  - 0005 (customers: "postal code formats aren't checked yet"), 0007 (orders snapshot
    the address), 0013 and 0015 (shipping zones and rate apps), 0023 and 0024 (the
    storefront, carts mapping onto UCP's `messages` with a `path`).
  - 0027 and 0031 (checkout and payment first: checks run before a payment starts).
  - 0029 (apps, extension points, `cart.validate` named as later), 0035 (agents).
  - 0038 (discounts: typed conditions and a pure engine, the shape the rules repeat).

## Why

Most platforms other than Shopify take any text as an address. The merchant finds out
when a parcel comes back, or when the carrier's API refuses the label. Hypha is the same
today:

1. **Almost nothing is checked.** `Address::new` requires a last name, a first line and a
   city, caps each field at 255 characters and checks the country. The postal code and
   the region are free text in every country: `1` is a Spanish postal code and
   `Kalifornia` a US state.
2. **One form fits every country.** The storefront shows "Postal code" and "Region" to a
   Spanish buyer, to a Japanese one and to one in Hong Kong, which has no postal codes.
3. **Carriers have rules the merchant can't express.** No PO boxes for courier services,
   a house number in the Netherlands, a phone number for UPS abroad, 35 characters per
   line for UPS and FedEx, a DHL Packstation's Postnummer, no military addresses. A
   merchant who knows them has nowhere to put them.
4. **Nothing can warn.** The cart has no messages: checkout refuses with one code or
   places the order. "This address may be undeliverable, did you mean …?" has nowhere to
   go, and neither has an app's opinion.
5. **Apps can't take part.** 0029 named `cart.validate` as a later extension point; there
   is none yet.

## Decisions

| | Decision |
| --- | --- |
| AV1 | **Four layers, one result shape:** format checks (built in, always on, §3), verification (optional, by an app, §5), checkout rules (built in, set up by the merchant, §6) and validation apps (an extension point, §7). Each produces the same messages (AV2) |
| AV2 | **Messages on the cart:** `{severity, code, path, message, source}`. `severity` is `error` or `warning`; **errors block checkout, warnings never do.** Rules and apps may warn as well as block (Shopify's Functions only block). `path` is a JSONPath into the cart's own response, so it maps straight onto UCP's `messages` (§4) |
| AV3 | **Setting an address still answers `422` for a format problem,** now listing every problem by field (`details.problems`). Address books (admin and account), locations and order edits do the same. Format *warnings* (a house number that seems missing) come back as messages on the cart. A softer `PUT` that stores the address and answers `200` with errors is §13's (§3.4) |
| AV4 | **A new leaf, `hypha-addresses`,** holding a snapshot of Shopify's `worldwide` address data (MIT, with Unicode CLDR data), generated into Rust and wasm-clean. It implements a core port, `AddressFormats`, that commands take, and serves each country's form: fields, order, labels and regions (`GET /storefront/address-formats/{country}`) (§3) |
| AV5 | **`province` holds the region's code** where the country has a list (`CA`, not "California"; `M`, not "Madrid"). Names, alternate names and codes are all accepted and normalised. Rows written before stay as they are and still load (§3.3) |
| AV6 | **Verification is the `address.verify` extension point,** answered by one app the merchant picks (like the search app). It's called when a cart's address is set, its answer is kept on the cart and copied to the order. A failure lets checkout go on. **By default verification only warns and suggests;** a merchant makes a verdict block with a rule (§5) |
| AV7 | **Checkout rules (`crule_…`) are typed data,** not an expression language: where the rule applies, up to 10 conditions that must all hold, then an error or a warning with the merchant's message on one field. The admin offers presets that fill a rule in, and a "Try an address" test. The engine is a pure function (§6) |
| AV8 | **`checkout.validate` v1** (0029's `cart.validate`, renamed): every installed app that declares it is asked when the address or the shipping choice changes and at checkout, and answers messages with paths. **The merchant decides per app whether its failure blocks checkout** (off by default, as Shopify's "Allow all customers to submit checkout") (§7) |
| AV9 | **Checkout refuses a cart with an error message** (`409 checkout_blocked`, every error in `details.messages`), **and so does starting its payment,** so no money is taken for a cart that can't be placed (§8) |
| AV10 | **The order keeps its address check** (the verification, the warnings it was placed with, and who marked it valid). The admin flags orders whose address needs review, staff may "Mark as valid", and **staff can edit an order's shipping address** before anything is fulfilled (§9) |
| AV11 | **Autocomplete is `address.suggest` and `address.resolve`,** answered by the app the merchant picks, called through the API so provider keys stay on the server, rate limited per client. The storefront's and the admin's address forms use it (§10) |
| AV12 | **A PO box meets a courier option as an error on the chosen option** ("DHL can't deliver to PO boxes: choose another option"), through a rule scoped to that option. Hiding the option is §13's (§6.4) |
| AV13 | **New scopes `checkout_rules:read` and `checkout_rules:write`** for rules and the checkout settings. Agents may hold both, `Apply` included (§11) |

## 1. What others do

### Shopify

The only platform with all the layers
([address collection](https://help.shopify.com/en/manual/checkout-settings/address-collection-preferences),
[order address validation](https://help.shopify.com/en/manual/orders/manage-orders/shipping-address-validation)):

- **Field checks always run** in every country: required fields, formats, lengths. A
  postal code that doesn't match the province must be fixed before checkout (Plus stores
  may turn that off).
- **"Validate shipping address"** is a checkbox: problems and suggestions appear in
  checkout, a suggestion only "when there is high confidence". Full coverage in 20
  countries, fewer suggestions elsewhere. It runs again on every order (API orders, draft
  orders, customer profiles too); the admin shows a warning staff can "Mark as valid"
  and an error ("very likely to cause failed delivery") they can't.
- **Autocomplete** is on by default in 23 countries. Plus stores may plug in their own
  provider (`purchase.address-autocomplete.suggest` and `.format-suggestion`: at most 5
  suggestions, then a second call turns one into an address).
- **Address format validation** (June 2026, Settings → Checkout → Checkout rules) blocks
  non-compliant addresses for every merchant, "across online and agentic checkout". It
  replaced the Checkout Blocks "Address Blocker".
- **The Cart and Checkout Validation Function API**
  ([reference](https://shopify.dev/docs/api/functions/latest/cart-and-checkout-validation)):
  a wasm function reads the cart (lines, products, buyer, delivery groups with their
  addresses and chosen option, the step: cart, checkout, completion) and the merchant's
  configuration from a metafield, and returns `errors: [{message, target}]` with targets
  such as `$.cart.deliveryGroups[0].deliveryAddress.zip`. **Every error blocks; there are
  no warnings.** Up to 25 per store. Per validation, the merchant picks whether a failing
  function blocks checkout (`blockOnFailure`). The merchant configures it in an admin UI
  extension that writes the metafield.
- **Delivery Customization** is a separate function type that hides, renames or moves
  shipping options; hiding options for PO boxes is its documented example.
- Shopify's maintained address data is
  [`worldwide`](https://github.com/Shopify/worldwide) (MIT): per country, the postal code
  pattern, the regions with codes, alternate names and postal code prefixes, whether a
  house number is required, the form's fields and labels.

### The others

- **Saleor** checks every address against Google's address data (`i18naddress`) and
  serves each country's rules (`addressValidationRules`). It has no synchronous hook for
  validating a checkout; `Checkout.problems` holds typed problems, some non-blocking.
- **Medusa v2**: nothing built in; a `validate` hook on `completeCartWorkflow` throws at
  completion, with no field targets.
- **commercetools**: nothing built in; a guide recommends calling Loqate or Google from the
  frontend. Its shipping methods take predicates (`shippingAddress.state in ("NSW")`).
- **Vendure**: block at a state change (`OrderProcess.onTransitionStart`).
- **WooCommerce**: a basic postcode check; plugins add errors through a hook, and one that
  throws is skipped.
- **BigCommerce, Magento**: apps inject scripts into the checkout page, so checks run in
  the browser only.

None lets a merchant write rules without code, none gives an app per-field errors, and
none has warnings that don't block.

### Rules merchants set up

Shopify's validation apps ([AddressReady](https://apps.shopify.com/addressready),
[HBN Address Rules](https://apps.shopify.com/hbn-address-rules),
[Qikify](https://help.qikify.com/en/article/checkout-validations-8x3gp1/)) are one
validation function each, reading a JSON list of rules. What they offer, per country or
all countries: no PO boxes, no military addresses; house number, postal code or phone
required; minimum and maximum length per field; forbidden characters; a field matching or
not matching a pattern; a postal code in a list or range. Each blocks with the merchant's
message under the field; one app adds info and warning levels. That's the shape of §6.

Carrier rules behind them:

- **PO boxes:** most courier services refuse them; found by pattern ("PO Box", "Postfach",
  "Apartado", "Boîte postale"…) or a provider's flag.
- **APO/FPO/DPO** (US military): city APO, FPO or DPO, state AA, AE or AP; only USPS
  delivers.
- **35 characters per line** (UPS, FedEx; DHL 35 to 45 by service).
- **A phone number** for UPS international, FedEx, DHL Express.
- **A house number** in the Netherlands, Germany, Belgium: the most common rule in apps.
- **DHL Packstation:** "Packstation" and its number in the street line, the 6–10 digit
  Postnummer in the second line.
- **Places outside the EU's VAT area found by postal code:** the Canary Islands (35, 38),
  Ceuta (51), Melilla (52), Heligoland, Büsingen, Livigno, Campione d'Italia, Mount Athos.
- **Remote-area surcharges** (Scottish Highlands and Islands): each carrier has its own
  list, so it's merchant data, never built in.

### Verification providers

Google Address Validation, Smarty, Loqate, USPS (US only, free, rate limited), EasyPost,
Shippo, and geocoders (HERE, Mapbox). Their answers share a common shape, which §5's port
takes:

- an overall verdict: deliverable, deliverable with fixes, missing an apartment, ambiguous,
  undeliverable;
- a status per field: confirmed, corrected, inferred, unconfirmed, missing;
- a suggested address;
- flags: PO box, residential, business, vacant.

They charge per lookup, so an answer is kept and reused for the same address (§5.3).

### What we take

- Shopify's three layers, its "suggest only when confident", its order flag with "Mark as
  valid", and its per-validation `blockOnFailure`.
- Messages that can warn, which Shopify's functions can't.
- Rules as typed data, which every no-code app ends up as.
- Shopify's address data, because it's MIT and maintained (Google's is CC-BY 4.0, which
  isn't on `deny.toml`'s list and needs attribution).

## 2. The model at a glance

```
              ┌────────────── cart.messages ───────────────┐
 address ──▶  format (hypha-addresses)       errors: 422 on PUT; warnings: messages
              verification (address.verify)  kept on the cart, copied to the order
              rules (hypha-checkout-rules)   evaluated on every read of the cart
              apps (checkout.validate)       asked on changes, kept on the cart
              └────────────────────────────────────────────┘
 checkout / start payment: any error ──▶ 409 checkout_blocked
```

| Piece | Where | Stored |
| --- | --- | --- |
| `AddressFormats` port, `AddressProblem`, `AddressFlags` | `hypha-core` (`address.rs`) | no |
| `Message`, `Severity`, `MessagePath`, `MessageSource` | `hypha-core` (`checks.rs`) | in the cart's and order's checks |
| `AddressVerification` (verdict, fields, suggestion, flags) | `hypha-core` | on carts and orders |
| Format data, `Formats` (implements the port), flag detection | `hypha-addresses` (new leaf) | compiled in |
| `CheckoutRule`, `CheckoutSettings`, the engine | `hypha-checkout-rules` (new leaf) | `checkout_rules`, `checkout_settings` |
| Points `address.verify`, `.suggest`, `.resolve`, `checkout.validate` | `hypha-extensions` | calls in `extension_calls` |
| The cart's check results | `hypha-cart` | `carts.address_check`, `carts.app_checks` |
| Composing the messages, enforcing them | `hypha-checkout` | no |
| The order's check | `hypha-orders` | `orders.address_check` |

## 3. Formats (AV3, AV4, AV5)

### 3.1 The data

`scripts/address-data.mjs <path to a worldwide checkout>` reads `data/regions/*.yml` and
writes `crates/hypha-addresses/src/generated/` (Rust `static` tables) with the commit it
read in a header. It's run by hand to take an update, reviewed like code. Per country:

- the fields the form shows, in order and per row (`edit` format), and which are
  required: the postal code isn't in every country (Hong Kong, the UAE), the region is
  required only where the country lists regions and its format has one;
- the postal code pattern (Ruby regex syntax, rewritten for the `regex` crate by the
  script, which fails on a construct it can't rewrite) and an example;
- the regions: code, name, alternate names, postal code prefixes;
- whether a house number is required (`building_number_required`);
- the labels: "State", "Province", "Prefecture", "County"; "ZIP code", "Postcode",
  "PIN". English only for now (§13).

The licence (MIT, with CLDR's Unicode licence for the names) goes beside the generated
files. Size matters on Workers: only these fields are generated, with no translations,
tax rates or time zones. The build step measures the wasm size before and after.

### 3.2 Checks

`Formats::check(&Address) -> Checked { address, problems, warnings, flags }` normalises
and checks in one pass:

| Check | Problem (`422`, AV3) | Warning (message) |
| --- | --- | --- |
| A field the country requires is empty | `{field, code: "required"}` | |
| The postal code doesn't match the pattern | `postal_code` `invalid_format` (with the example) | |
| The region isn't one of the country's | `province` `unknown_region` | |
| The postal code belongs to another region | `postal_code` `region_mismatch` | |
| A postal code in a country without them | | dropped quietly |
| The country needs a house number and line 1 has no digit | | `house_number_missing` on `line1` |

Normalising: postal codes are upper-cased with spaces collapsed; regions become their
code; nothing else is rewritten (§5's suggestions do that, with the shopper's consent).

`problems` stop the write; `warnings` become messages when the address is a cart's (§4).
Every write of an address goes through the port: carts, address books (admin and
account), locations, and order edits (§9). `Address::new` in core stays as it is (the
structural rules), and **restoring a stored address never runs the format check**, so
rows written before this note still load. Such an address is checked again the next time
it's written. Checkout doesn't re-run the format check: the address was checked when it
was set.

### 3.3 Regions

`province` keeps its column and name; what changes is its content. Where a country lists
regions, `Formats` turns a name, an alternate name ("Calif.") or a code into the code
(`CA`), case-insensitively, and answers the code. Responses add `province_name`
(`"California"`) beside it, so nobody has to look the name up. Where a country has no
list, the text stays as entered.

This is the hard-to-reverse part (§14): from now on `province` means a code for those
countries. Shipping zones and tax regions by region (§13) will match on it.

### 3.4 Errors on the wire

`invalid_address` (422) gains `details.problems: [{field, code, message}]`, every
problem at once, so a form marks each field. A cart's `PUT …/shipping-address` keeps
refusing (AV3): the softer `200` with error messages, which would let a storefront save a
half-typed address and show its errors, is §13's.

### 3.5 Forms

`GET /storefront/address-formats/{country}` and `GET /admin/address-formats/{country}`
answer a country's form, public data cached for a day:

```json
{
  "country": "US",
  "rows": [["first_name", "last_name"], ["company"], ["line1"], ["line2"],
           ["city", "province", "postal_code"], ["phone"]],
  "required": ["last_name", "line1", "city", "province", "postal_code"],
  "labels": { "province": "State", "postal_code": "ZIP code" },
  "postal_code_example": "90210",
  "regions": [{ "code": "AL", "name": "Alabama" }, "…"],
  "autocomplete": true
}
```

`autocomplete` says whether the store has an autocomplete app (§10). The storefront's
checkout and account forms and the admin's address forms render from it: fields in the
country's order, the region as a select when there's a list, the right labels.

### 3.6 Flags

`Formats` also answers `AddressFlags`, which rules read (§6) and which verification
(§5) adds to:

- `po_box`: a pattern per language over `line1` and `line2` ("PO Box", "P.O.B.", "Post
  Office Box", "Postfach", "Apartado de correos", "Boîte postale", "Casella postale",
  "Postbus", "Skrytka pocztowa", "Poštanski pretinac"…), word-bounded so "Boxer Street"
  isn't one. The list is data in the crate, added to as countries ask.
- `military`: US, city APO/FPO/DPO or state AA/AE/AP.
- `parcel_locker`: DE, "Packstation" or "Postfiliale" with a number.
- `non_latin`: a letter outside the Latin script in any field.

## 4. Messages (AV2)

```json
{
  "severity": "error",
  "code": "po_box_not_allowed",
  "path": "$.shipping_address.line1",
  "message": "We can't ship to PO boxes with DHL Express. Choose another option.",
  "source": { "type": "rule", "rule": "crule_…" }
}
```

- **`severity`:** `error` blocks checkout; `warning` never does.
- **`code`:** snake_case. Built-in codes are a contract like error codes; a rule's code is
  the merchant's (optional, `rule` by default); an app's is the app's.
- **`path`:** RFC 9535 JSONPath into the cart response as the caller receives it, from a
  closed set: `$` (the cart), `$.email`, `$.shipping_address`,
  `$.shipping_address.<field>`, `$.shipping_groups[<i>]` (its chosen option),
  `$.lines[<i>]`. The paths of a response are valid in that response; UCP's `messages`
  take them as they are.
- **`source`:** `format`, `verification`, `rule` (with its ID), `app` (with its key). The
  storefront may style them alike; staff and apps can tell them apart.
- **`message`:** plain text for the shopper. Built-in ones are English until translations
  (§13); a rule's is the merchant's; an app's is the app's (at most 500 characters).

Carts (`GET` and every write that answers the cart, storefront and admin) gain
`messages`, computed when the response is built:

1. the format warnings of the address (§3.2), recomputed: cheap;
2. the verification kept on the cart, turned into messages (§5.4);
3. the rules, evaluated now (§6.3): pure and cheap, so they follow every change of lines,
   shipping or market;
4. each validation app's last answer, kept on the cart with what it was sent (§7.3).

A cart without an address has no address messages: a rule about the address waits for
one.

## 5. Verification (AV6)

### 5.1 The point

`address.verify` v1. `SCOPE` `customer_data:address` (the app reads the street), `READ_ONLY`,
`TIMEOUT` 3 s, `ON_FAILURE` `Fallback`. Request:

```json
{
  "address": { "company": null, "line1": "1600 Amphitheatre Pkwy", "line2": null,
               "city": "Mountain View", "province": "CA", "postal_code": "94043",
               "country": "US" },
  "context": "cart"
}
```

Names and phone aren't sent: no provider needs them. `context` is `cart` or `order`.
Answer:

```json
{
  "verdict": "deliverable_with_fixes",
  "precision": "premise",
  "fields": { "postal_code": { "status": "corrected" }, "line1": { "status": "confirmed" } },
  "suggested": { "line1": "1600 Amphitheatre Pkwy", "city": "Mountain View",
                 "province": "CA", "postal_code": "94043-1351", "country": "US" },
  "flags": { "po_box": false, "residential": false, "business": true, "vacant": false },
  "reference": "provider's own ID"
}
```

- `verdict`: `deliverable`, `deliverable_with_fixes`, `missing_subpremise`, `ambiguous`,
  `undeliverable`, `unknown`.
- `precision`: `country`, `locality`, `street`, `premise`, `subpremise`.
- `fields`: per field, `confirmed`, `corrected`, `inferred`, `unconfirmed`, `missing`.
- `suggested`: only when the app is confident (Shopify's rule); same country, or the point
  refuses the answer (`check`).
- `flags`: each optional; an absent one is unknown.

### 5.2 Which app

`CheckoutSettings::verification_app` (`PATCH /admin/checkout-settings`): an installed app
whose manifest declares `address.verify` and whose grant has the scope, like the search
app. None: no verification, and no messages from it.

### 5.3 When it's called, and what's kept

- When a cart's address is set and passes the format check, before the cart is written.
  The answer is kept on the cart (`Cart::address_check`: the digest of the address sent,
  the answer, the app, when) and the same write stores it.
- Keyed by a SHA-256 of the normalised address fields sent: setting the same address
  again (the storefront sends it on every step) asks nobody. When the answer has a
  `suggested` address, its digest is kept as `deliverable` too, so a shopper accepting
  the suggestion costs no second lookup.
- A failure (timeout, error, a refused answer) keeps `verdict: unknown` with the failure,
  and the extension call log has the details. Checkout goes on.
- Checkout uses what's kept; it doesn't ask again.
- Order edits ask again (§9).

### 5.4 Messages from a verification

| Verdict | Message (warning) |
| --- | --- |
| `deliverable` | none |
| `deliverable_with_fixes` | `address_corrections_suggested` on `$.shipping_address`, with `suggested` on the cart |
| `missing_subpremise` | `apartment_missing` on `$.shipping_address.line2` |
| `ambiguous` | `address_ambiguous` on `$.shipping_address` |
| `undeliverable` | `address_undeliverable` on `$.shipping_address` |
| `unknown` | none (a failure isn't the shopper's problem) |

The cart response gains `shipping_address_check: {verdict, suggested, flags, checked_at}`
so the storefront can show "Did you mean …?" with the suggestion, and "Use as entered"
dismisses it client-side (a warning never blocks, so nothing needs recording).
Verification never blocks on its own; a merchant who wants undeliverable addresses
refused writes a rule (§6, `verification_is`).

## 6. Checkout rules (AV7, AV12)

### 6.1 The aggregate

`CheckoutRule` (`crule_…`), in `hypha-checkout-rules`:

- `name` (staff's), `enabled`, `position` (the order the admin lists them in);
- `applies_to`: `countries` (`include` or `exclude` a list, or all), `markets`,
  `channels`, `shipping_options` (empty: any; set: only when one of them is chosen);
- `conditions`: 1 to 10, all must hold (§6.2);
- `outcome`: `severity` (`error`/`warning`), `message` (1 to 300 characters), `path` (one
  of §4's paths, `$.shipping_address.line1` by default for address conditions), `code`
  (optional, snake_case);
- `Versioned`, `Stamped`; events `checkout_rule.created`, `.updated` (fields),
  `.deleted`, routed to apps with `checkout_rules:read`.

`CheckoutRule::new`/`update`/`restore` check every rule at once (`invalid_checkout_rule`
with `{path, code, message}` per problem, like discounts). At most 100 rules per store
(`409 too_many_checkout_rules`), so evaluating them all on every read of a cart stays
cheap.

### 6.2 Conditions

Typed, each with its own fields (`type` tags them):

| Condition | Holds when |
| --- | --- |
| `field_missing {field}` / `field_present {field}` | an address field (or `email`) is empty / isn't |
| `field_matches {field, pattern, case_insensitive}` / `field_not_matches` | a regex (`regex` crate syntax, at most 300 characters, compiled when saved) matches |
| `field_contains {field, values}` / `field_not_contains` | any of up to 50 strings is in the field, case-insensitive |
| `field_longer_than {field, length}` / `field_shorter_than` | characters, not bytes |
| `postal_code_in {prefixes, ranges, codes}` / `postal_code_not_in` | up to 1,000 entries; ranges compare codes of equal length, numerically when both are digits |
| `region_in {regions}` / `region_not_in` | the region's code (§3.3) |
| `address_is {flag}` / `address_is_not` | a flag: `po_box`, `military`, `parcel_locker`, `non_latin` (§3.6), `residential`, `business`, `vacant` (verification's; unknown counts as not) |
| `verification_is {verdicts}` | the kept verdict is one of them (`unknown` included, for "verification failed") |
| `cart_contains {products, categories}` / `cart_not_contains` | a line's product, or a category it's in (read only when a rule names categories) |
| `subtotal_at_least {amounts}` / `subtotal_below` | per currency; a currency not listed doesn't hold |
| `weight_at_least {grams}` / `weight_below` | the shipped lines' weight; lines without a weight count 0 |

`field` is one of the address fields or `email`. "Has a phone for UPS abroad" is
`applies_to.shipping_options: [UPS Worldwide]`, `field_missing {phone}`, error on
`$.shipping_address.phone`. A rule's conditions never call out: everything they read is
on the cart or kept with it.

### 6.3 The engine

`evaluate(input: &RuleInput, rules: &[CheckoutRule]) -> Vec<Message>` is pure. `RuleInput`
is plain values: the address (normalised) and its flags (format's merged with
verification's), the verdict, country, market, channel, the chosen option per group, the
lines (product, categories, quantity, weight), subtotal and currency. Each enabled rule
whose `applies_to` matches and whose conditions all hold adds its message; a rule about
the address on a cart without one is skipped. Rules on a shipping option put their
message on that group's path by default (`$.shipping_groups[0]`).

### 6.4 PO boxes and couriers (AV12)

The preset "No PO boxes for these options" is `applies_to.shipping_options: [the
courier's]`, `address_is {po_box}`, an error on the group. The shopper sees it under the
option and picks another; checkout refuses until then. Hiding the option instead (Shopify's
Delivery Customization) is §13's.

### 6.5 Presets and testing

The admin's "Add rule" offers presets that fill in a rule the merchant then edits:

- No PO boxes (all options, or chosen ones);
- No military addresses;
- House number required (NL, DE, BE…: line 1 must contain a digit);
- Phone required (for chosen options);
- Lines of at most 35 characters (UPS, FedEx);
- No Canary Islands, Ceuta or Melilla (ES, `postal_code_in {prefixes: [35, 38, 51, 52]}`);
- Refuse undeliverable addresses (`verification_is [undeliverable]`).

`POST /admin/checkout-rules/test` takes an address and optionally a market, lines and a
shipping option, and answers the messages every layer would give (format, rules, and
verification when `verify: true`, which costs a lookup). A draft rule can be tested
before it's saved (`rule` in the body).

## 7. Validation apps (AV8)

### 7.1 The point

`checkout.validate` v1, `SCOPE` `carts:read`, `READ_ONLY`, `TIMEOUT` 2 s. Its failure
policy is the merchant's (§7.2). Request:

```json
{
  "step": "address",
  "cart": {
    "id": "cart_…", "market": "mkt_…", "channel": "chan_…", "country": "DE",
    "currency": "EUR", "customer": "cus_…",
    "email": "…", "shipping_address": { "…": "…" },
    "shipping_address_check": { "verdict": "deliverable", "flags": { "…": "…" } },
    "lines": [{ "variant": "var_…", "product": "prod_…", "sku": "…", "quantity": 2,
                "unit_price": { "…": "…" }, "weight_grams": 300 }],
    "shipping_groups": [{ "option": "shipopt_…", "code": "dhl_express", "provider": null }],
    "subtotal": { "…": "…" }, "total": { "…": "…" }
  }
}
```

`step` is `address`, `shipping` or `checkout`. The address and email are redacted per the
app's grant (`customer_data:address`, `:name`, `:phone`, `:email`), as `shipping.rates`'
destination is. Answer: `{messages: [{severity, code, path, message}]}`, at most 20; the
point's `check` refuses a path outside §4's set or a message over 500 characters, which
counts as a failure.

### 7.2 Which apps, and their failure

Every installed app whose manifest declares `checkout.validate` and whose grant holds
`carts:read` is asked, all at once (up to 5 apps; a sixth isn't installed for the point:
`409 too_many_validation_apps` when its grant is approved). Each has a setting in
`CheckoutSettings::validation_apps`: `enabled` (on when installed) and
`block_on_failure` (off by default). An app that fails is skipped when it doesn't block;
when it does, it adds `validation_unavailable` (error, `$`, "We can't check your order
right now. Try again in a moment."). The extension framework's `ON_FAILURE` gains a third
value, `Configured`, for a point whose caller applies a setting.

### 7.3 When, and what's kept

- `address` when the address is set, `shipping` when an option is chosen, `checkout` at
  checkout and when a payment starts.
- Each answer is kept on the cart (`Cart::app_checks`: per app, the digest of the request
  it answered, its messages, when). An app is asked again only when what it would be sent
  changed; at checkout, a stale answer is asked again and the fresh one decides.
- Changing lines doesn't call apps (adding to the cart stays fast); the answer goes stale
  and checkout asks.

### 7.4 Writing one

`@hyphacommerce/app` gains `defineExtensionHandler` types for the point (generated from
`openapi/extensions.json` as the others) and a `message(path, …)` helper. The apps guide
gets a worked example: "no alcohol to these postal codes".

## 8. Checkout (AV9)

`checkout` and `start_cart_payment` compute the cart's messages as §4 does, asking stale
validation apps first (§7.3). Any `error` refuses with **`409 checkout_blocked`**,
`details.messages` holding every error (warnings left out), before anything is reserved
or any payment started. Checkout's existing checks (lines, shipping, payment) keep their
codes and come first.

A payment committed before a rule changed (a merchant edits a rule while a shopper pays)
is released as today when the order can't be placed.

## 9. Orders (AV10)

- **The check is kept:** `Order::address_check` (migration): the verification as kept on
  the cart, the warnings the cart had when it was placed, and `marked_valid {by, at}`.
  Orders before this note have none.
- **"Needs review":** an order whose check has an `ambiguous` or `undeliverable` verdict
  or an address warning, until marked valid. The orders list gains `address:
  needs_review` as a filter and a badge; the order page a card with the verdict, the
  flags, the warnings and the suggestion.
- **Mark as valid:** `POST /admin/orders/{id}/address-check/mark-valid` (`orders:write`):
  records who and when; `order.address_marked_valid`.
- **Editing the shipping address:** `PUT /admin/orders/{id}/shipping-address`
  (`orders:write`), refused once anything is fulfilled (`409 order_fulfilled`). The
  format check runs (`422`), verification is asked again (`context: order`), and the new
  check replaces the old (`marked_valid` cleared). It records
  `order.shipping_address_changed` (the address redacted per the receiving app's grant).
  Shipping isn't repriced: the order keeps what was charged. The admin offers "Use
  suggested address" when there's a suggestion.

## 10. Autocomplete (AV11)

### 10.1 The points

- `address.suggest` v1 (`SCOPE` `customer_data:address`, `READ_ONLY`, `TIMEOUT` 1 s,
  `Fallback`): `{query, country, session, limit}` → `{suggestions: [{id, label,
  description}]}`, at most 5 (Shopify's number). `session` is the client's token for
  providers that bill per session (Google's Places).
- `address.resolve` v1 (same): `{id, session}` → `{address}`: the parts, which then go
  through the format check like any typed address.

`CheckoutSettings::autocomplete_app` names the app; it may be the verification app or
another.

### 10.2 The routes

- `GET /storefront/address-suggestions?q=&country=&session=` and
  `GET /storefront/address-suggestions/{id}?session=` (publishable key; `q` 3 to 200
  characters). Rate limited per client address (`Rule::AddressSuggestIp`, generous: one
  call per keystroke after debouncing) and per session (`Rule::AddressSuggestSession`), so
  a scraper can't spend the merchant's lookups.
- The same under `/admin/` for staff forms.
- No app: `404 autocomplete_unavailable`; the forms know beforehand from
  `autocomplete` (§3.5). A failing app answers no suggestions: the shopper types.

### 10.3 The forms

The storefront's checkout and account address forms put a combobox on line 1 (Astryx's,
debounced 250 ms, through a server action, so the publishable key stays on the server):
picking a suggestion resolves it and fills the form, which the shopper can still edit.
The admin's order and customer address forms do the same.

## 11. Scopes, agents and privacy (AV13)

- `checkout_rules:read` reads rules and the checkout settings; `checkout_rules:write`
  changes them. Agents may hold both, `Apply` included: a rule is easy to undo.
- Testing an address (`…/test`) needs `checkout_rules:read`; with `verify: true`,
  `checkout_rules:write` (it spends a lookup).
- The cart's `shipping_address_check.suggested` is an address: redacted by
  `customer_data:address` on the admin surface (`redact.rs`, `Family::Order`, which
  covers carts), as the order's check is. Messages are shopper-facing text and aren't
  redacted, but built-in ones never quote the address.
- Autocomplete queries aren't stored. The extension call log records calls without
  bodies, as it does.

## 12. API, events, admin

### 12.1 Routes

| Route | Scope |
| --- | --- |
| `GET /storefront/address-formats/{country}`, `GET /admin/address-formats/{country}` | publishable key / any admin caller |
| `GET /storefront/address-suggestions`, `…/{id}` (and `/admin/`) | publishable key / `customers:read` |
| `GET`/`POST /admin/checkout-rules`, `GET`/`PATCH`/`DELETE …/{id}` | `checkout_rules:read`/`write` |
| `POST /admin/checkout-rules/test` | `checkout_rules:read` (§11) |
| `GET`/`PATCH /admin/checkout-settings` | `checkout_rules:read`/`write` |
| `POST /admin/orders/{id}/address-check/mark-valid` | `orders:write` |
| `PUT /admin/orders/{id}/shipping-address` | `orders:write` |

Carts gain `messages` and `shipping_address_check`; orders gain `address_check`;
`invalid_address` gains `details.problems`; checkout and payments gain
`409 checkout_blocked`.

### 12.2 Events

`checkout_rule.created`/`updated`/`deleted`, `checkout_settings.updated`,
`order.shipping_address_changed` and `order.address_marked_valid` (new order event types).

### 12.3 Admin

- **Settings → Checkout** (a new page): verification app, autocomplete app, validation
  apps with "Block checkout when this app fails", and **Checkout rules**: the list
  (enabled toggle, drag to reorder), "Add rule" with presets, the editor (where it
  applies, conditions, outcome, a live "Try an address" panel).
- **Orders:** the "Needs review" filter and badge, the address check card, "Mark as
  valid", "Edit address" with autocomplete and the suggestion.
- **Customers and locations:** address forms rendered from the country's format, with
  autocomplete.

### 12.4 Storefront

- Checkout and account forms from the country's format (§3.5), with autocomplete (§10.3).
- Field problems from `422` under each field; cart messages under the field their path
  names (an unknown path at the top), errors in red, warnings in amber.
- "Did you mean …?" with the suggestion and "Use as entered".
- `checkout_blocked` shows its messages the same way.

## 13. Later

- **A softer `PUT` for the cart's address** (AV3): store an address with format errors
  and answer `200` with them as messages, so a form can save as the shopper goes and
  checkout blocks. It changes what clients get for a bad address, so it waits for after
  the beta.
- **Hiding, renaming and ordering shipping options** by address or cart (Shopify's
  Delivery Customization), as a rule outcome and an extension point (AV12).
- **Shipping zones and tax regions by region and postal code**, on AV5's codes: state
  tax in the US, remote-area surcharges by postal code, the Canary Islands outside EU
  VAT.
- **Billing addresses** on carts and orders, checked the same way.
- **Translations** of built-in messages, labels and region names (worldwide has them),
  and of rules' messages (with the languages work).
- **Verifying address books** and a bulk check of past orders.
- **Functions:** a wasm `checkout.validate` with no network hop (0029 §11).
- **A first-party verification and autocomplete app** (Google Address Validation and
  Places, or Smarty for the US), in its own repository like Stripe's.
- **A typed pickup point** (Packstation, parcel lockers) instead of patterns in line 1.

## 14. Hard to reverse

| What | Choice | Why it's hard |
| --- | --- | --- |
| The message shape and its paths (AV2) | `{severity, code, path, message, source}`, JSONPath into the cart response, a closed set of paths | a public contract for storefronts, apps and UCP |
| `province` as a code (AV5) | the region's code where the country lists regions, `province_name` beside it | changes what the field means; zones and tax will match on it |
| Format failures are `422` on writes (AV3) | every write of an address refuses them | addresses accepted before may be refused when edited; §13's softer `PUT` would change the answer again |
| Checks kept on carts and orders | `carts.address_check`, `carts.app_checks`, `orders.address_check` (JSON columns) | storage schema |
| Four new extension points | `address.verify`, `.suggest`, `.resolve`, `checkout.validate` v1 bodies | a reshaped body is a new version |
| `ON_FAILURE::Configured` | the caller applies the merchant's setting | changes the extension framework's contract |
| Scope names (AV13) | `checkout_rules:read`/`write` | grants name them |
| The data source | Shopify's `worldwide`, generated in | another source would change codes and patterns |

## 15. Layering

| Crate | Change | Edge |
| --- | --- | --- |
| `hypha-core` | `AddressFormats` port, `AddressProblem`, `AddressFlags`, `AddressVerification`, `checks::Message` | none |
| `hypha-addresses` (new leaf) | the data, `Formats` | core only |
| `hypha-checkout-rules` (new leaf) | rules, settings, the engine | core only |
| `hypha-extensions` | the four points | none new |
| `hypha-cart`, `hypha-customers`, `hypha-inventory` (locations), `hypha-orders` | take `&impl AddressFormats` on address writes; cart and order keep their checks | none new (the port is core's) |
| `hypha-checkout` | messages, enforcement, the calls through ports the host gives (`AddressVerifier`, `CheckoutValidators`, like `RateProvider`) | **new: → `hypha-checkout-rules`** |
| `hypha-http` | routes, `HostAddresses` over `HostExtensions`, redaction | → both new leaves (hosts may) |

The only new edge between domain crates is checkout → checkout-rules (a workflow on a
leaf, which the layering allows; flagged as CLAUDE.md asks).

## 16. Build order

One branch per step, stacked with `gh stack` from `feat/address-validation`:

1. **Formats:** core's port and types, `hypha-addresses` with the generator and the data,
   checks and flags, `invalid_address`'s problems, every address write through the port,
   the formats route, the storefront's and admin's forms by country, e2e.
2. **Messages:** core's `Message`, carts' `messages` (format warnings), checkout's and
   payment's `checkout_blocked`, the storefront showing them.
3. **Rules:** `hypha-checkout-rules`, its table, routes, events, scopes, the engine in the
   messages, the admin's Checkout page with presets and the test panel, the agent grant
   form.
4. **Verification:** the point, `checkout_settings`, the cart's check, suggestions in the
   storefront, the order's check, "Needs review", "Mark as valid", editing an order's
   address.
5. **Autocomplete:** the points, the routes and their rate limits, the storefront's and
   admin's comboboxes.
6. **Validation apps:** the point, `Configured`, per-app settings, the cart's app checks,
   `@hyphacommerce/app`'s types and the guide's example.
7. **Seed and docs:** the demo store gets rules (no PO boxes for express, house number in
   NL/DE), a test validation app in e2e, the site's docs page.

## As built

### Step 1: formats

- **The data:** Shopify/worldwide at `15d9289e57cb49f99a4f10e392c4114cd72cc06c`
  (2026-10-28), MIT with CLDR's Unicode licence (`crates/hypha-addresses/src/generated/LICENSE.md`).
  `scripts/address-data.mjs` reads only its YAML: 244 countries, 19 distinct form layouts,
  1,124 regions. Labels come from each country's `en.yml`, over `_default/en.yml`'s.
  Only one table is generated (`countries.rs`, about 200 KB of source); the release
  Worker grew by 134,895 bytes raw (32,319,572 → 32,454,467) and 673 bytes gzipped.
- **Worldwide's own rules, followed:** a postal code is required when `zip_requirement`
  is `required`, or absent with a `zip_regex`; `recommended` and `optional` aren't. The
  region is shown when the country's `format.edit` has it, listed when it has zones, and
  required unless `province_optional`. Countries with one postal code filled in
  (`zip_autofill_enabled`) don't show it, so `PostalCode` has no "filled" case.
- **Patterns:** `\d` becomes `[0-9]` (Ruby's is ASCII), runs of spaces collapse like the
  codes compared against them, unanchored patterns are anchored, and a negative
  lookahead (Denmark's and five others') becomes a separate pattern a code must not match
  at its start. A test holds every country's example to its pattern.
- **Region against postal code:** compared on the code without spaces and hyphens, also
  without a leading country prefix (`ES-28039`); a code no region's prefixes place is
  never a mismatch, since the data isn't complete everywhere.
- **Where the check runs:** carts' shipping address, customers' address books (admin and
  account) and locations, through core's `AddressFormats::validate`, which reports
  `Address::new`'s problems and the country's together. The store's legal address and
  suppliers' addresses keep `Address::new`'s structural rules only: neither is shipped
  to. Format warnings (`house_number_missing`) are computed by `check` and reach carts as
  messages in step 2.
- **Problems on the wire:** `code` is `required`, `too_long`, `invalid_format`,
  `unknown_region` or `region_mismatch`; a wrong country is still `invalid_country_code`
  / `unknown_country_code` alone. Messages use the country's labels ("Enter a valid ZIP
  code, like 90210").
- **Forms:** `GET …/address-formats/{country}` answers every shown field's label (not
  only the overrides), `postal_code` (`none`, `required`, `recommended`, `optional`) and
  `house_number_required` beside §3.5's fields; an unknown country is `404`. The
  storefront's checkout and account forms load the page's country's form on the server;
  the admin's customer address dialog gets the forms of the book's countries from its
  page and asks for a newly chosen country's through a server action.
- **Test data:** Croatian and Slovenian postal codes are now required, so e2e's and the
  route tests' addresses gained them; the seed's Milan shopper gained `MI`.

### Step 2: messages

- **The shape:** core's `checks.rs` has `Message`, `Severity`, `MessageCode` (snake_case,
  a letter first, at most 64 characters; built-in codes are associated constants checked
  by a test), `MessagePath` (§4's closed set, an enum written and read as the JSONPath
  string; an index is plain digits without a leading zero, within `u32`) and
  `MessageSource` (`format`, `verification`, `rule {rule}`, `app {app}`). Text is trimmed
  and 1 to 500 characters (`invalid_message`, `invalid_message_code`,
  `invalid_message_path`). All of them serialise in the wire shape both ways, so steps 4
  and 6 store them as they are. `CheckoutRuleId` (`crule_…`) is in core already, for the
  rule source.
- **Composing:** `hypha_checkout::cart_messages(checks, &PricedCart)`, over a
  `CartChecks` parameter (the host's `AddressFormats` for now; any `AddressFormats` is
  one), so the verification, rules and apps of later steps add a source without changing
  the commands' signatures again. A checked-out cart has no messages: nothing about it
  can change, and its address is the order's.
- **On the wire:** every cart response (storefront, and the admin's `PUT
  /admin/carts/{id}/customer`) has `messages`, computed in `CartResponse::from`.
- **Enforcing:** `ready` (checkout's checks, shared with `start_cart_payment`) computes the
  messages; the caller refuses on them last, so every existing code keeps its place:
  checkout after the committed-payment checks (`payment_required`, `payment_pending`,
  `payment_failed`, `cart_payment_committed`), starting a payment after
  `payment_method_offline`, `cart_nothing_to_pay` and `cart_payment_committed`. Both are
  before anything is reserved or any payment created. `checkout_blocked` gives committed
  money back like `total_changed` (§8's rule changed while paying). The checkout and
  payment routes document their `409`s.
- **The storefront:** `lib/messages.ts` places a message by its path, and
  `components/cart-messages.tsx` shows it (errors red, warnings amber):
  - a field's message is its input's status until the field is edited;
  - the address's and the email's are shown beside them;
  - a group's goes under its options, and a line's under the line (checkout summary, cart
    lines);
  - `$` and unknown paths go at the top of the checkout and the cart.

  A `checkout_blocked` answer re-renders the checkout, whose cart then shows the errors
  where they belong.
- **Tested:** no source makes an error yet, so `refuse_errors` is unit-tested with one
  injected; checkout's tests use a `CartChecks` that warns and check out anyway, and the
  route tests and e2e set a Dutch or German address without a house number.

### Step 3: rules

- **The leaf:** `hypha-checkout-rules` on core alone. A rule's parts (`RuleAppliesTo`,
  `RuleCondition`, `RuleOutcome`) are plain data that derive serde and, behind an
  `openapi` feature, `ToSchema`: the API's shape and the stored JSON are one, as
  discounts' JSON is, so a reshape needs a migration. `CheckoutRule::new`/`update`/
  `restore` check the whole draft (`invalid_checkout_rule`, `details.problems: [{path,
  code, message}]`). Limits beside §6.2's: 250 per list (countries, markets, channels,
  options, regions, products and categories together), 50 amounts, a field length up to
  255, postal codes of at most 20 characters.
- **Patterns:** compiled when saved, with a size limit; after loading, on first use, since
  compiling up to a thousand patterns on every read of a cart costs more than the rules. A
  pattern a later `regex` couldn't compile never holds.
- **Paths:** a rule's message may point anywhere in §4's set but a line (whose index
  changes as the shopper edits): `path_not_allowed`. Without one: the chosen option's
  group for a rule on options, else the field the first condition reads (line 1 for a
  flag, the address for a verdict, the first matching line for `cart_contains`), else `$`.
- **Order:** `position` is the list's order; moving a rule renumbers every rule densely in
  one commit (`CheckoutRuleRepo::update_many`, all or `StaleVersion`), each with
  `checkout_rule.updated` naming `position`. A new rule goes last.
- **In the messages:** `cart_messages(ports, checks, cart)` reads the rules on every
  composition (one query of at most 100 rows) and lines' categories only when an enabled
  rule names some (`hypha_cart::product_categories`, the price lists' read, so checkout
  gains no edge but the approved one). `PricedCart` gained `channel` and `LineDetails`
  `shipping_weight` for the input. The cart response's messages are filled in
  `cart_response`, which already read the store.
- **Verification facts** (`VerifiedFacts`: verdict, `po_box`, `residential`, `business`,
  `vacant`) are in `RuleInput` already, `None` until step 4 keeps them: `verification_is`
  and the verification flags don't hold meanwhile. Core gained `AddressVerdict` for the
  conditions to name.
- **`CheckoutSettings` waits for step 4:** each of its fields (verification app,
  autocomplete app, validation apps) arrives with the step that uses it, so there's no
  empty settings route in between.
- **The test route** (`POST /admin/checkout-rules/test`) answers the format's problems
  (and nothing more, as a cart would refuse the address), or the normalised address, its
  flags and the messages: the format's warnings, then every enabled rule's, with a draft
  `rule` tried in place of `rule_id` (or last) even while it's off, its messages marked
  `draft` (a draft not saved yet names `crule_000…0`). Lines are read by variant for their
  product, categories and weight; the subtotal is given, not priced.
- **The admin:** Settings → Checkout lists the rules (enable, move up and down, delete),
  "Add rule" offers §6.5's presets (`lib/checkout-rules.ts`), and the editor (new from a
  preset, or a saved rule's page) has every condition's fields, the outcome, and "Try an
  address" beside it. The agent grant form and the apps' scope labels name
  `checkout_rules`.
