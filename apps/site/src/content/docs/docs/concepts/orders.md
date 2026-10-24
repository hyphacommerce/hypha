---
title: Orders and fulfilment
description: From a checked-out cart to shipped parcels, cancellations and refunds.
sidebar:
  order: 8
---

A cart becomes an **order** at checkout. The order copies everything that mattered at that
moment (prices, taxes, shipping, addresses), so later changes to products or prices never
rewrite it.

Each line keeps its unit price, the compare-at price shown with it and the price list that
set it. When [discounts](/docs/concepts/discounts/) applied, the order keeps a copy of each
one, and every line and shipping line has its `subtotal` (before discounts), its share of
each discount and its `total` (after them). Tax and refunds go by the total.

## Fulfilment

A **fulfilment** (`ful_…`) is what left one location for an order: which lines and how many
of each, with a carrier and tracking number when there is one. An order can ship in as many
fulfilments as it needs, from different locations. The order counts what has shipped and
says where it stands: unfulfilled, partly fulfilled or fulfilled.

Creating a fulfilment, the order's counts and the stock taken from the location are one
write: they can't disagree.

## Cancelling

Cancelling a fulfilment puts its units back on its location's shelf. Cancelling an order
voids its pending payments and returns its reserved stock.

## Money

An order's **financial status** (pending, authorized, paid, partially refunded, refunded,
voided) comes from its payments; see [Payments](/docs/concepts/payments/). Staff can record
money received for orders paid outside Hypha, such as cash on delivery.

## Emails to the buyer

The buyer gets an email when the order is placed, and, when staff choose to notify them,
when it's cancelled or a fulfilment ships or changes. Every message is recorded, with its
status, and can be sent again from the order.
