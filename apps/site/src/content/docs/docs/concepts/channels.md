---
title: Sales channels
description: Where products are sold, apart from who buys them.
sidebar:
  order: 3
---

Markets decide *who* buys and at what price; **sales channels** decide *where* products are
sold: your online store, a shopping agent, a marketplace, a kiosk.

Every store starts with an **Online Store** channel. Others are `agentic` (sold through AI
shopping agents) or `custom`.

## Publishing

A product is sold in a channel once it's **published** there. A new product is published
nowhere: publishing is a deliberate step, product by product or in bulk.

## Publishable keys

A storefront calls the storefront API with a **publishable key** (`hypha_pk_…`), and each
key belongs to one channel. The key decides which channel a request is for, so a storefront
can never ask for another channel's products, and nothing a shopper sends can change it.
Staff and secret keys see every channel.

Payment methods can be limited to channels too: cash on delivery on your own storefront,
but not through a marketplace.
