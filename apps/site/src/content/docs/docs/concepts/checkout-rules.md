---
title: Addresses and checkout rules
description: Address formats by country, messages on carts, the merchant's checkout rules, and apps that verify, suggest and validate.
sidebar:
  order: 12
---

A wrong address is the costliest mistake a checkout lets through: the parcel comes back,
or the carrier refuses the label. Hypha checks addresses in four layers, and each of them
speaks the same language: **messages** on the cart, each pointing at what it's about.

| Layer | What it does | Set up by |
| --- | --- | --- |
| Formats | Each country's fields, postal code pattern and regions | Built in, always on |
| Verification | Deliverable or not, a suggested correction, flags such as PO box | An app the merchant picks |
| Checkout rules | Conditions on the address and the cart, ending in an error or a warning | The merchant, in the admin |
| Validation apps | Any check an app wants to make, with its own messages | Installed apps |

Autocomplete, through an app too, helps the shopper type the address in the first place.

## Formats

Every country has its own address form: which fields it shows and in what order, which
are required, what the region and the postal code are called ("State" and "ZIP code" in
the US, "Prefecture" in Japan), the postal code's pattern, and the list of regions. Hypha
carries this for 244 countries, generated from Shopify's
[worldwide](https://github.com/Shopify/worldwide) data (MIT, with Unicode CLDR's names).

`GET /storefront/address-formats/{country}` (and `/admin/…`) answers a country's form, so
a storefront renders the right one:

```json
{
  "country": "US",
  "rows": [["first_name", "last_name"], ["company"], ["line1"], ["line2"],
           ["city", "province", "postal_code"], ["phone"]],
  "required": ["last_name", "line1", "city", "province", "postal_code"],
  "labels": { "province": "State", "postal_code": "ZIP code", "…": "…" },
  "postal_code": "required",
  "postal_code_example": "90210",
  "house_number_required": true,
  "regions": [{ "code": "AL", "name": "Alabama" }, "…"],
  "autocomplete": false
}
```

`postal_code` is `required`, `recommended`, `optional` or `none` (Hong Kong has none).
`autocomplete` says whether the store has an autocomplete app.

### Problems

Every write of an address is checked against its country's format: a cart's shipping
address, customers' address books (from the admin or the shopper's account) and
locations. A problem refuses the write with `422 invalid_address`, listing every problem
at once so a form can mark each field:

```json
{
  "code": "invalid_address",
  "details": {
    "problems": [
      { "field": "postal_code", "code": "invalid_format", "message": "Enter a valid ZIP code, like 90210" },
      { "field": "province", "code": "required", "message": "Enter a state" }
    ]
  }
}
```

The codes are `required`, `too_long`, `invalid_format`, `unknown_region` and
`region_mismatch` (a postal code that belongs to another region).

### Regions as codes

Where a country lists regions, `province` holds the region's code: `CA`, not
"California"; `M`, not "Madrid". A name, an alternate name or a code is accepted, in any
case, and stored as the code. Responses add `province_name` beside it. Where a country has
no list, the region is kept as entered. Postal codes are upper-cased with their spaces
collapsed; nothing else is rewritten.

Addresses stored before these checks still load as they are; they're checked the next
time they're written.

## Messages

Every cart response has `messages`:

```json
{
  "severity": "error",
  "code": "po_box_not_allowed",
  "path": "$.shipping_groups[0]",
  "message": "Express couriers can't deliver to PO boxes. Choose Standard or enter a street address.",
  "source": { "type": "rule", "rule": "crule_…" }
}
```

- **`severity`:** an `error` blocks checkout; a `warning` never does.
- **`path`:** a JSONPath into the cart response itself, from a closed set: `$` (the
  cart), `$.email`, `$.shipping_address`, `$.shipping_address.<field>`,
  `$.shipping_groups[<i>]` (its chosen option) and `$.lines[<i>]`. A storefront shows each
  message under what its path names, and anything else at the top. The paths map as they
  are onto the Universal Commerce Protocol's `messages`.
- **`source`:** `format`, `verification`, `rule` (with its ID) or `app` (with its key).
- **`code`:** built-in codes are a contract like error codes; a rule's code is the
  merchant's, an app's the app's.

The format adds warnings of its own: `house_number_missing` when the country needs a
house number and the first line has no digit.

### Checkout blocked

Checkout, and starting the cart's payment, refuse a cart with an error message:
`409 checkout_blocked`, with every error in `details.messages`. Both refuse after their
other checks and before any stock is reserved or any payment started, so no money is
taken for a cart that can't be placed.

## Checkout rules

Carriers have rules a format can't know: no PO boxes for couriers, a phone number for
international express, 35 characters per address line on labels. A **checkout rule**
(`crule_…`) says one of them as typed data, without code:

- **where it applies:** countries (all, only these, or all but these), markets, channels,
  and shipping options (only while one of them is chosen);
- **conditions,** 1 to 10, which must all hold: a field missing or present, matching a
  pattern or containing a word, longer or shorter than a length; the postal code in a list
  of prefixes, ranges or codes; the region; a flag (`po_box`, `military`,
  `parcel_locker`, `non_latin`, and verification's `residential`, `business`, `vacant`);
  the verification's verdict; what the cart contains; its subtotal; its weight;
- **the outcome:** an error or a warning, the merchant's message, the path it points at
  (by default the chosen option for a rule on options, else the field its first condition
  reads), and an optional code (`rule` by default).

A store has up to 100 rules, evaluated in their order on every read of a cart, so a rule
follows every change of lines, shipping or market at once.

The admin's **Settings → Checkout** lists them and offers presets to start from: no PO
boxes (for every option, or chosen ones), no military addresses, a house number required,
a phone required for chosen options, lines of at most 35 characters, no Canary Islands,
Ceuta or Melilla, and refusing undeliverable addresses.

`POST /admin/checkout-rules/test` tries an address against them, and against a draft rule
before it's saved: it answers the format's problems, or the normalised address, its flags
and every message the rules give (with `verify: true`, the verification app's too, which
costs a lookup).

Rules and the checkout settings need the `checkout_rules:read` and `checkout_rules:write`
scopes. Agents may hold both.

## Apps

**Settings → Checkout** (`GET`/`PATCH /admin/checkout-settings`) picks the apps that take
part. Each must declare the [extension point](/docs/guides/apps/) and be granted its scope.

### Verification

The **verification app** (`verification_app`) answers `address.verify` when a cart's
address is set: a verdict (`deliverable`, `deliverable_with_fixes`, `missing_subpremise`,
`ambiguous`, `undeliverable`, `unknown`), the status of each field, a suggested address
when it's confident, and flags. The answer is kept on the cart and copied to the order;
setting the same address again asks nobody, and neither does accepting the suggestion.

Verification only warns: `address_corrections_suggested` (with the suggestion in the
cart's `shipping_address_check`, for "Did you mean …?"), `apartment_missing`,
`address_ambiguous`, `address_undeliverable`. An app that fails or times out lets checkout
go on. A merchant who wants undeliverable addresses refused writes a rule on the verdict.

### Autocomplete

The **autocomplete app** (`autocomplete_app`) answers `address.suggest` (up to 5
suggestions while the shopper types) and `address.resolve` (one suggestion as an
address). Storefronts call them through Hypha, so the provider's keys stay on the server:

- `GET /storefront/address-suggestions?q=&country=&session=`
- `GET /storefront/address-suggestions/{id}?country=&session=`

Both are rate limited per client and per `session` (the form's token, which providers
bill by). A resolved address comes back normalised with any `problems` the format sees,
for the shopper to fix. Without an app they answer `404 autocomplete_unavailable`.

### Validation apps

Every installed app that declares `checkout.validate` and is granted `carts:read` is asked
when the address or the shipping choice changes and at checkout, up to five apps at once.
Each answers messages with paths, as rules do. Its answer is kept on the cart and asked
again only when what it would be sent changed; checkout always asks a stale one again.

Per app, the merchant decides whether it's asked and whether its failure blocks checkout
(off by default); a blocking app that fails adds `validation_unavailable`. The
[apps guide](/docs/guides/apps/) has a worked example.

## Orders

An order keeps its **address check**: the verification as it was at checkout and the
address warnings the cart had. An order whose verdict is `ambiguous` or `undeliverable`,
or which was placed with an address warning, **needs review** until staff mark it valid:
the admin's orders list has the filter (`?address=needs_review`) and a badge, and the
order page shows the check with "Mark as valid"
(`POST /admin/orders/{id}/address-check/mark-valid`).

Staff can change an order's shipping address until anything is fulfilled
(`PUT /admin/orders/{id}/shipping-address`): the format checks it, verification is asked
again, and the new check replaces the old. It stays in the order's country, since shipping
and tax aren't priced again. Events: `order.shipping_address_changed` and
`order.address_marked_valid`.
