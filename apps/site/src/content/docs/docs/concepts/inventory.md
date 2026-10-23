---
title: Inventory and locations
description: Stock per variant and location, and how checkout reserves it.
sidebar:
  order: 5
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

Stock changes are events like any other: apps and the storefront's cached pages follow
them through webhooks.
