---
title: Payments
description: Payment methods, providers, and payments taken before the order exists.
sidebar:
  order: 9
---

## Payment methods

A **payment method** is what a shopper picks at checkout: "Card", "Bank transfer", "Cash on
delivery". Each has a name, instructions for the buyer, the markets and channels it's
offered in (all of them when empty), and a **provider** that does the work:

- `manual`, built in: no money moves through Hypha; the buyer follows the instructions and
  staff record the payment when it arrives;
- an app (`app_<name>`) that answers the payment extension points;
- a spore (`spore_<name>`), such as the Stripe spore.

A cart can only choose a method offered for its market and channel, by a provider this
deployment has. A cart whose total is zero needs no payment at all.

## Paying before the order exists

With an online provider, the payment starts while it's still a cart: the storefront asks
Hypha to start it, the provider answers with what the buyer does next (a card form, a
redirect), and checkout places the order only once the money is held or captured. If the
order can't be placed after all (stock ran out meanwhile), the money is given back.

When the shopper never comes back to the storefront after paying, the provider's report
(a webhook) completes the checkout on its own.

With a manual method, the order and a pending payment are created together at checkout.

## Payments

A **payment** (`pay_…`) is one attempt to collect an order's total. It moves through
`pending`, `authorized`, `processing`, `paid`, `failed`, `voided`, `partially_refunded`
and `refunded`, only in the ways the rules allow. Staff capture, void and refund from the
admin; the provider does the work and reports back.
