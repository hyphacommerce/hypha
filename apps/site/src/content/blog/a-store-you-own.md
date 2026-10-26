---
title: A store you own
description: Hypha is an open-source commerce platform you run yourself, so your store's rent stops growing with your sales. What it does for a merchant, and why I built it.
date: 2026-10-26
tags: [announcements]
---

If you sell online, you probably rent your store. The platform takes a monthly fee, a cut of
each sale through its payment gateway, and a few more subscriptions for the apps that fill
in what's missing. It's a fair deal when you start. It gets expensive as you grow, and by
then moving is hard, because the platform holds your products, your orders and your
customers in a shape only it understands.

Hypha is the other way of doing it. It's open source, it runs on a server you choose (a
small one costs about five euros a month, or Cloudflare's network), and it's yours: the
code, the data, the servers. There's no plan to upgrade to and no fee per sale.

## What you get

A full store, not a toolkit:

- **A catalogue built for real ranges.** Products with options and only the variants you
  sell, not every combination. Categories, images, rich descriptions and search with
  filters.
- **Selling in several countries.** Markets set the country, currency and taxes. Each
  market can have its own prices and its own sales.
- **Prices and promotions.** Scheduled sales and clearances, quantity breaks, wholesale
  prices for a customer group. Discount codes and automatic discounts, buy X get Y,
  volume tiers, free shipping, with limits on how often each applies.
- **Checkout and payments.** Card payments through Stripe, or bank transfer and cash on
  delivery. Shipping zones and rates, or live rates from a carrier app.
- **Orders and fulfilment.** Fulfil from more than one location, with tracking numbers,
  cancellations and refunds. Stock is tracked per location.
- **Customers.** They sign in with a code sent to their email, no passwords to forget.
  They see their orders, addresses and marketing choices, and can download or erase their
  data.
- **Staff.** Roles with exact permissions, two-factor sign-in, invitations.
- **Emails.** Order confirmations, shipping notices and the rest, with templates you can
  edit and preview.

And a part most platforms don't have yet: **AI assistants that help run the store**. You
can connect Claude, ChatGPT or another assistant to your admin and ask it to adjust
prices, update stock or tidy up product descriptions. Every change it wants to make is
shown to you exactly as it would happen, and nothing is applied until you approve it.

## What it costs

The software is free. A store on a small server costs the server, around five euros a
month, plus your payment provider's fees. The same store on Shopify costs the plan, the
transaction fee on top of the card fee, and the apps. The [home page](/) has a calculator
with the numbers for your sales volume.

## Who it's for

Hypha is a good fit if your product range is complicated, if you sell in several
countries, or if you've been quoted a platform's "Plus" tier and winced. You'll want a
developer, or an agency, to set it up and give the storefront your look. After that, the
admin is yours to run.

It's an early release: I'd call it a beta. It works end to end, but it hasn't carried a
real shop's orders yet, and I'm looking for the first merchants to run on it. If that's
you, the setup and the first integrations are on me, in exchange for your feedback and a
story I can tell.

## Why I built it

I wanted a store that belongs to the merchant, that a developer can read and extend, and
that stays affordable when the business does well. The platforms I'd worked with got the
first part wrong by design, and the rest followed from it.

You can try the demo at [demo.hyphacommerce.com](https://demo.hyphacommerce.com), read the
[docs](/docs), or write to hello@hyphacommerce.com. And if Hypha saves you a platform fee,
[sponsoring it](https://github.com/sponsors/hyphacommerce) is what keeps it moving.
