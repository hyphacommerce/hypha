---
title: "Hypha 0.1.0 beta 4: purchasing, addresses and checkout rules"
description: Buy stock from suppliers and know what it cost you. Check addresses by country, let apps verify and autocomplete them, and set the rules a cart must meet before checkout.
date: 2026-11-02
tags: [releases]
release: true
version: 0.1.0-beta.4
changes:
  - kind: Added
    text: Suppliers, purchase orders sent by email, receiving in parts, landed costs and a moving average unit cost.
  - kind: Added
    text: Incoming stock on every inventory level, transfers between locations, and a stock ledger of every change.
  - kind: Added
    text: Address forms and checks for every country, with regions as codes and `province_name` beside them.
  - kind: Added
    text: Messages on carts, each pointing at the field, line or shipping group it's about; an error refuses checkout with `checkout_blocked`.
  - kind: Added
    text: Checkout rules, set up in the admin under Settings → Checkout.
  - kind: Added
    text: The extension points `address.verify`, `address.suggest`, `address.resolve` and `checkout.validate`.
  - kind: Breaking
    text: An address's `province` is now the region's code (`CA`, not `California`), and an address the country's format refuses is `422 invalid_address`.
---

This beta adds two areas a shop runs into in its first weeks: where stock comes from, and
what happens when a shopper types an address the carrier can't use.

## Purchasing

Hypha now covers buying stock as well as selling it. You keep your **suppliers** with
their contacts, payment terms and lead times, and a catalogue of what each sells you at
what cost, in the supplier's currency.

A **purchase order** goes to one supplier for one location. Hypha emails it to the
supplier, keeps a print view, and counts what's still to come. Goods arrive in parts, and
units the supplier got wrong can be rejected. Shipping, duties and fees are spread over
the lines by value or by quantity, so every unit gets a **landed cost** in your own
currency. Each item keeps a moving average of what its stock cost you, and a fee that
arrives a week later still revalues the stock it belongs to.

What's ordered shows as **incoming** beside each location's stock, with the date it's
expected. **Transfers** move stock between your own locations, and every change to a count
is written to a **stock ledger** with its reason.

The [purchasing docs](/docs/concepts/purchasing) go through a purchase from start to
finish.

## Addresses and checkout rules

Every address Hypha stores now goes through its **country's format**: the fields that
country uses, in its order, with its labels, postal code pattern and regions. The data
comes from Shopify's open `worldwide` dataset. The storefront's checkout and the admin
lay their forms out from it, so a Japanese address doesn't get a US form. Regions are
stored as codes, so `California`, `california` and `CA` all become `CA`.

Carts now answer **messages**. Each one points at what it's about, such as a field of the
address, a line or a shipping group, and the storefront shows it there. A warning is shown
and the shopper can still check out. An error refuses checkout.

Merchants write those messages themselves with **checkout rules**, set up in the admin
without code. Some examples:

- no PO boxes or parcel lockers with the express options;
- a house number in Germany and the Netherlands;
- nothing over 30 kg to the islands;
- a minimum order for one market.

The editor has a panel where you can try a rule on an address before turning it on.

Three kinds of apps can take part:

- a **verification** app checks that the address exists, suggests a corrected one, and
  can flag a PO box or a business;
- an **autocomplete** app suggests addresses while the shopper types line 1;
- **validation** apps check the whole cart with their own logic. A merchant chooses
  whether checkout waits for an app that's down.

Orders whose addresses look doubtful show under "Needs review" in the admin, where staff
can fix the address or mark it as valid.

The [addresses and checkout rules docs](/docs/concepts/checkout-rules) cover each part,
and the [apps guide](/docs/guides/apps) has a worked validation app.

## Upgrading

Migrations run on start, as before. Two changes reach API clients:

- `province` holds the region's code. Responses add `province_name` beside it for display.
- Addresses the country's format refuses are now `422 invalid_address`, with every problem
  by field in `details.problems`. Addresses saved before keep loading as they are.

The [demo](https://demo.hyphacommerce.com) has suppliers, purchase orders and two checkout
rules to try. You can sign in to the [demo admin](https://demo-admin.hyphacommerce.com)
as `demo@hyphacommerce.com` with the password `hypha-demo`.
