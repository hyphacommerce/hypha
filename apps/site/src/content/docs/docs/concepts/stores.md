---
title: Stores
description: One store per deployment, and what belongs to it.
sidebar:
  order: 1
---

A Hypha deployment runs **one store**. The API creates it on its first start, with an owner
account and a default stock location, and everything else (products, markets, orders,
customers, staff) belongs to it.

Running several stores means running several deployments. They share nothing but the
code, which keeps every store's data, settings and limits apart.

## Store details

In the admin's settings, or through `GET` and `PATCH /admin/store`:

- the store's name, contact details and address, shown in emails;
- its time zone, which emails and reports use for dates;
- its branding for emails (logo, colours) and the address buyers' emails link to for an
  order.

The admin's **setup guide** walks a new store through the steps that make it ready to
sell: a market, a product, stock, publishing, a publishable key and a payment method.

## Settings the operator owns

Some settings belong to whoever runs the deployment rather than the merchant, and live in
`hypha.config.toml`: the address the API binds to, trusted proxies, rate limits, sign-in
lifetimes, background job timers, the email sender and whether AI agents may write.
Secrets (the database URL, the secret key) are environment variables only. The API's
`config` command prints the settings in effect.
