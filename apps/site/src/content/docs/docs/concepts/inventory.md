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
