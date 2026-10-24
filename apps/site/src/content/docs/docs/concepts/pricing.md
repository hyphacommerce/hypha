---
title: Prices and sales
description: Price lists, sales and schedules, compare-at prices, quantity breaks, group prices and the bulk price editor.
sidebar:
  order: 3
---

A variant has a **base price**. **Price lists** change what shoppers pay for it in a
market: a market's own regular prices, a sale, a clearance, wholesale prices for a customer
group. They set the price a shopper sees *before* the cart. Reductions that depend on the
cart (codes, "buy X get Y", free shipping) are [discounts](/docs/concepts/discounts/).

## Price lists

A price list (`pl_…`) has:

- a **name** (unique, ignoring case) and a description;
- a **currency**, fixed once it's created;
- a **kind**: `sale` (a reduction, shown struck through against the regular price) or
  `override` (the regular price where it applies, called "Regular prices" in the admin);
- a **status**, `draft` or `active`, and an optional **window** (`starts_at`, `ends_at`).
  From the status and the window, a list reads as draft, scheduled, active or expired;
- an optional **adjustment**: a percentage of the base price (in basis points: `-2000` is
  20% off) for all products, listed products, or listed categories (with every category
  below them);
- **fixed prices** for variants, each with an optional compare-at price and quantity
  breaks;
- an **audience**: customer groups whose members get its prices, or everyone when it's
  empty.

A list prices nothing until it's on a market's catalog. A catalog can hold any number of
lists, all in the market's currency, and a list can sit on several catalogs (one list for
every euro market).

### Which price wins

For a variant in a market, every **live** list on the catalog that prices the variant is a
candidate. A list is live when it's active, inside its window and its audience includes the
buyer. It prices the variant when it has a fixed price for it or its adjustment covers the
product.

- **The lowest price wins.** There's no priority to manage: a wholesale list can never
  raise a group's price above a public sale, because the buyer gets whichever is lower.
  Ties go to the older list.
- **The base price counts only when no live list prices the variant.** It must be in the
  market's currency: Hypha doesn't convert currencies.
- A list with no adjustment prices only its fixed variants. That's what lets a sale list
  hold just the items on sale.

A markup (`override`, +20%) doesn't beat a cheaper list on the same catalog. Put a markup
on the market's catalog alone, and put together only the lists that are meant to compete.

Rounding is half away from zero, on whole minor units.

## Compare-at prices and sales

A **compare-at price** is the "was" price shown struck through. A variant has one (it must
be above its base price), and so can each fixed price. What shoppers see struck through
depends on the list whose price won:

| The price came from | Struck through |
| --- | --- |
| a `sale` list | its fixed price's compare-at, else the regular price: the lowest `override` price, or the base price |
| an `override` list | its fixed price's compare-at, else the variant's compare-at, adjusted by the list (or none, when the list hides it) |
| the base price | the variant's compare-at price |

A compare-at is shown only when it's above the price. A product showing one is **on sale**:
search facets and filters on it (`filter=on_sale:true`), and Hypha's storefront has a sale
page built on that filter.

The commonest sale, "20% off these", needs no compare-at to type: the regular price is the
compare-at, and the list's kind says it's a sale. A clearance is the same: a `sale` list on
a category, with no end date.

In the EU, a reduction must be shown against the lowest price of the last 30 days. Hypha
doesn't keep that history yet: set the compare-at to that price yourself.

## Schedules

Prices are worked out at the moment of the request, so a cart priced one second after a
sale starts has the sale. Nothing has to run for carts and checkout to be right.

What's cached has to follow: search's documents, a storefront's cached pages, apps'
webhooks. A job checks every minute for windows that opened or closed and records
`price_list.started` and `price_list.ended`. Search re-syncs on them, and Hypha's
storefront refreshes its prices.

## Quantity breaks

A fixed price can carry up to 10 **breaks**: "€8 each from 10". Each break's minimum
quantity is 2 or more, rising, and each price is below the one before. They count per
variant within a cart line. Each candidate list is priced at the line's quantity, then the
lowest wins. Product pages list the winning list's breaks, and search shows the price for
one.

## Group prices

A list whose audience names customer groups prices only for buyers in one of them: a
signed-in customer, or a cart linked to one. Their lists compete with the public ones under
the same rule, so a wholesale buyer still gets a public sale when it's cheaper.

Carts and checkout always price for the buyer. Cached storefront pages and search are
shared by every shopper, so they show public prices.

## In the admin

Price lists are under **Markets → Price lists**, with a status for each. A new list starts
from a template:

- **Sale**: a reduction on a category, from one date to another;
- **Clearance**: a category at a reduction, with no end date;
- **Market prices**: a market's own regular prices, an adjustment and some fixed prices;
- **Wholesale**: fixed prices and quantity breaks for a customer group.

A list's page holds its settings and the markets it applies in. A product's page shows its
price in each market and the list that wins it (`GET /admin/products/{id}/prices`), with
every other candidate and why it lost. That route also answers "what does a wholesale buyer
in Austria pay for this?", for staff and for agents.

### The bulk price editor

**Open price editor**, on a list's **Prices** tab, shows a full-screen spreadsheet of
products with their variants beneath, one column per price. The base price, the variant's
compare-at and the price the list gives are shown read-only; when the price comes from the
adjustment, it's greyed. The fixed price, its compare-at and the quantity breaks are
edited in place. Filter by product, by category, or show fixed prices only.

It works like a spreadsheet:

- **Mouse**: click a cell to select it, click it again (or double-click) to edit; drag or
  Shift+click to select a range. A breaks cell opens its own editor.
- **Keys**: type to replace a cell, Enter or F2 to edit it, Enter and Tab to commit and
  move, Escape to cancel, arrows to move (Shift extends the selection, Ctrl/Cmd jumps to
  the edge), Delete to clear.
- **Copy and paste** with Ctrl/Cmd+C and Ctrl/Cmd+V, to and from any spreadsheet.
  Ctrl/Cmd+D fills down, Ctrl/Cmd+A selects everything.
- **Bulk actions** on a selection: set it to an amount, set it to a percentage of the base
  price, or clear it.

Changes stay in the editor until you **Save** (or press Ctrl/Cmd+S), and leaving with
unsaved changes asks first. Saving sends them in batches of up to 1,000. A row the API
refuses keeps its change and shows why, and the rest are saved.

### CSV export and import

**Export CSV** writes one row per variant: `handle`, `sku`, `variant_id`, `options`,
`base_price`, `fixed_price`, `compare_at`, then `break_<n>_qty` and `break_<n>_price` for
each break.

**Import CSV** uploads the file to the store's storage (up to 20 MB), not through a form.
The admin reads it from there and matches each row to a variant: by variant ID, else SKU,
else handle and options. It shows a preview before anything changes: rows changed,
unchanged, unmatched and invalid, each with its reasons. Applying sends only the changed
rows, in steps, and lists any row the API refused. The upload is deleted when the dialog
closes, and any left behind are removed after a day.

## The API

- `POST` / `PATCH` / `DELETE /admin/price-lists[/{id}]`. A list on a catalog can't be
  deleted (`409 price_list_in_use`). `GET /admin/price-lists` filters by kind, status,
  market and customer group.
- `PUT /admin/price-lists/{id}/fixed-prices` sets up to 1,000 fixed prices per request,
  and answers every bad entry at once, with its index. `DELETE` removes them, and `GET`
  filters by product, category and words.
- `PUT /admin/catalogs/{id}/price-lists` sets a catalog's lists, and
  `POST` / `DELETE /admin/catalogs/{id}/price-lists/{price_list}` add or remove one.
- Storefront variants carry `price`, `compare_at_price`, `on_sale` and `price_breaks`.
  Search hits carry `compare_at` and `on_sale`, and cart lines carry `compare_at_price`.
  Orders keep each line's compare-at and the list that priced it.
- Events: `price_list.created`, `.changed`, `.deleted`, `.started` and `.ended`, and
  `catalog.price_lists_added` / `_removed`.

Staff without `pricing:write` see the editor read-only. The scopes are `pricing:read` and `pricing:write`, plus `markets:write` to put a list on a
catalog.
