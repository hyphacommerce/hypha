---
title: Discounts
description: Codes and automatic discounts, buy X get Y, volume tiers, free shipping, combining them, and usage limits.
sidebar:
  order: 4
---

A **discount** (`disc_…`) reduces a cart: a code a shopper types, or an automatic one that
applies when the cart qualifies. [Price lists](/docs/concepts/pricing/) set what a product
costs before the cart; discounts depend on what's in it.

## What a discount is

- **A method**: `codes` (shoppers enter one of its codes) or `automatic` (it applies on its
  own). The method can't change after it's created.
- **A title**, shown to shoppers beside what it takes off.
- **A kind**, which says what it takes off (below).
- **Conditions** that must all hold, each matching any of its values: markets, sales
  channels, customer groups, listed customers, signed in, a minimum subtotal per market, a
  minimum quantity.
- **Combinations**: which classes of discount it may apply with (below).
- **Limits**: total uses, and once per customer.
- **A status** (`draft` or `active`) and an optional window, like a price list's.

Every discount has a **summary**, one sentence built from its data: "10% off Grow kits ·
orders over €50 · until 31 Oct". The admin, the cart, the order emails and agents all show
the same words.

### Kinds

- **Standard**: one to three rewards, each reducing one target:
  - **items**: all products, or listed products, variants or categories (with the
    categories below them), optionally leaving out items already on sale;
  - **the subtotal**;
  - **shipping**, optionally up to a cap.

  Each reward has a value: a **percentage**, an **amount** per market, or a **fixed price**
  per unit (items only; 0 makes them free). An amount on items is taken off each unit,
  spread across the items, or off the cheapest units.
- **Buy X get Y**: buy a quantity (or an amount) of some items, get a quantity of others at
  a percentage off (100% is free) or an amount off each, repeating while the cart
  qualifies, up to a number of times per order. The free item must be in the cart.
- **Tiered** ("Volume"): the more of the items, or the higher their subtotal, the bigger
  the reduction, in up to 10 rising tiers.

Amounts are per market (`{"mkt_…": {"amount": 500, "currency": "EUR"}}`), each in that
market's currency. A discount with no amount for the cart's market doesn't apply there. In
a market whose prices include tax, an amount includes tax too, as the shopper sees prices.

## How a cart is discounted

1. **Product discounts** reduce item lines, each computed on the line's price and added up
   within the class, so 10% and 20% on the same line make 30%. The order they're applied
   in never changes the total.
2. **Order discounts** reduce the subtotal after product discounts. Their amount is split
   over the lines in proportion to what each has left, to the cent: leftover cents go to
   the largest remainders, so the shares always add up exactly.
3. **Shipping discounts** reduce the chosen shipping rate, priced for the subtotal after
   the discounts above. "Free shipping over €50" and a 10% order discount agree on whether
   the cart is over €50. An order gets at most one shipping discount.

Nothing goes below zero. Tax is charged on what each line costs after its discounts.

## Combining

Each discount has **classes**, `product`, `order` or `shipping`, from what its rewards
reduce (buy X get Y and tiers are `product`), and says which classes it **combines with**.
Two discounts apply together only when each combines with every class of the other.

Of every set of discounts that may apply together, Hypha picks the one where **the buyer
pays least**, counting the lines and the shipping. It compares no discount at all, each
discount alone, and combinations built from each one. So a discount never makes a cart cost
more: 5% off €52 would leave €49.40 plus €5 shipping under a "free over €50" rate, so that
5% isn't applied. Ties go to fewer discounts.

There are at most 100 live automatic discounts per store.

## Codes

A discount's codes are rows of their own: as many as it needs, typed in or generated in
bulk with a prefix (`WELCOME-…`, at least 8 random characters after it). Codes are unique
in the store, and matched trimmed and case-insensitive. Each can have a use limit of its
own and be deactivated; generated batches can be exported as CSV.

A cart holds up to 5 codes. An unknown, inactive or not yet started code is refused
(`422 discount_code_invalid`), the same error for each, so codes can't be guessed from the
errors, and entering codes is rate limited per client and per cart. A known code is kept
even when it doesn't apply yet, and the cart says why for each:

| State | Meaning |
| --- | --- |
| `applied` | it's in the chosen set |
| `not_eligible` | a condition fails (market, group, signed in…), named in `condition` |
| `minimum_not_met` | the subtotal or quantity is short, by `needed` |
| `no_matching_items` | nothing in the cart is targeted |
| `blocked_by_other` | a better combination leaves it out, naming that `discount` |
| `no_saving` | with it, the buyer would pay no less |
| `limit_reached` | its uses are taken |
| `not_available_in_market` | it has no amount for this market |
| `expired` | its discount has ended |
| `invalid` | the code or its discount was removed or switched off |

## Usage and checkout

Uses are counted only when an order is placed, in the same write as the order. A limit
can't be overrun by shoppers checking out at once: one of them takes the last use, and the
other's checkout answers `409 discounts_changed` with the cart's new total and code states.
Nothing is placed and any payment started is given back, so a buyer never pays a total
they didn't see. Once per customer counts a guest by their email's customer. Cancelling an
order doesn't give its uses back.

## On orders

An order keeps a copy of every discount applied: its title, code, summary and what it took
off. Each line has its `subtotal` (before discounts), its `discounts` (each discount's
share) and its `total` (after them, what tax and refunds go by), and so does each shipping
line. The order's `discount_total` adds them up. The order emails list the discounts under
the totals.

## In the admin

**Discounts** is in the Store section. A new discount starts from a template: Amount off
products, Amount off order, Buy X get Y, Volume or Free shipping. The form previews the
summary as you edit and shows which live discounts your combination choices affect. A
discount's page has its settings, a **Codes** tab (search, add, generate, export,
deactivate) and **Usage** (the orders it was used on and how much it took off; it needs
`customers:read` and `orders:read`).

## In the storefront

Hypha's storefront has a code field in the cart drawer and on the cart page. Each code
shows its state as a message, and a discounted line shows what each discount took off, with
its total before them struck through. Product pages and cards show **promotions**: live automatic discounts that may
reach the product ("Buy a grow kit, get a syringe free"), on storefront product reads with
a `country` and on search hits.

## The API

- `GET` / `POST /admin/discounts`, `GET` / `PATCH` / `DELETE /admin/discounts/{id}`.
  Every rule a discount breaks is answered at once (`422 invalid_discount`, with
  `details.problems: [{path, code, message}]`).
- `…/{id}/codes` (list, add, change, remove), `…/codes/generate`, `…/codes/export` (CSV)
  and `…/{id}/usage`. `POST /admin/discounts/summary` gives a draft's summary before it's
  saved.
- `POST` / `DELETE /storefront/carts/{id}/discount-codes[/{code}]` add and remove codes;
  every cart answer carries `discounts`, `discount_total` and `discount_codes`.
- Events: `discount.created`, `discount.updated` and `discount.deleted`; `order.placed`
  carries `discount_codes` and `discount_total`.

The scopes are `discounts:read` and `discounts:write`. Creating a discount is one of the
tools admin [agents](/docs/guides/agents/) get directly.
