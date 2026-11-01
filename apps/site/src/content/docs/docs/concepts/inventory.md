---
title: Inventory and locations
description: Stock per variant and location, and how checkout reserves it.
sidebar:
  order: 7
---

Stock is counted per variant, per **location**: a warehouse, a shop, a supplier who ships
for you.

## Locations

Every store has a location from its first start, named "Default". Add more as you need
them, and say which markets each serves, so a cart is only promised stock that can reach
it.

## Inventory items

A variant whose stock you track has an **inventory item**, with a **level** per location:
how many are on hand. A variant without one is never out of stock (made to order, digital,
a service).

When a shopper checks out, the order **reserves** the stock it takes, in the same write
that creates the order, so two shoppers can't buy the last unit twice. Fulfilling the
order takes the units out of the location they ship from; cancelling gives them back.

## Stock history

Every change to a count leaves a row in the item's **history**
(`GET /admin/inventory-items/{id}/movements`), written in the same transaction as the
change, so no change goes unrecorded. Each row says how many units moved and how many were
left, why (`counted`, `sold`, `restocked`, `damaged`, `shrinkage`, `promotion`,
`correction`, `other`), what caused it (a fulfilment, a person) and who made it. Setting a
count records `counted`; an adjustment takes a `reason` and a `note`. At each location, the
rows add up to what's on hand.

Stock changes are events like any other: apps and the storefront's cached pages follow
them through webhooks.

## Unit cost

An item can keep what one unit cost you (`unit_cost`), in the store's **base currency**
(`PATCH /admin/store` with `currency`, or the setup wizard). Set it by hand with
`PUT /admin/inventory-items/{id}/cost` (`purchasing:write`); receiving purchase orders keeps
it as a moving average. Costs go to 4 decimals past the minor unit, as a decimal string of
minor units: `{"amount": "0.35", "currency": "EUR"}` is €0.0035.

- Every row of the history carries the cost of its units then, so the cost of goods sold
  and the stock's value never move when the cost changes later. A cost change is a row of
  its own (`cost_adjustment`, no location) with the cost before and after.
- Each order line keeps the cost of its units when the order was placed, for the order's
  margin.
- Once a cost is recorded, the base currency can't change (`409 store_currency_locked`).
- Cost is shown only to callers holding `purchasing:read`, staff included; elsewhere the
  fields are absent and listed in the object's `redacted`. Apps get
  `inventory_item.cost_changed` only with `purchasing:read`.

