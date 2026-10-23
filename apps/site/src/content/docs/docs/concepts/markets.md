---
title: Markets, catalogs and price lists
description: Who you sell to, what they can buy and what they pay.
sidebar:
  order: 2
---

Three things decide what a shopper sees and pays: the **market** they're in, that market's
**catalog**, and the catalog's **price list**. This follows Shopify's model of markets.

## Markets

A market is a set of countries that share a currency: "Croatia" in EUR, "United Kingdom"
in GBP, "Western Balkans" in EUR. A country belongs to at most one market. A shopper's cart
is in one market, chosen from their country, and every price in it is in that market's
currency.

Currencies are any ISO 4217 currency, each with its own minor units: amounts are always
whole numbers of the smallest unit (`2490` with `EUR` is €24.90; `1200` with `JPY` is
¥1,200). Hypha never uses floating-point numbers for money.

## Catalogs

Each market has one catalog: the products its shoppers can buy. A product missing from a
market's catalog doesn't exist for that market's shoppers.

## Price lists

A catalog can have a price list, in the market's currency. A variant's price in that market
is:

1. the price list's **fixed price** for that variant, when it has one;
2. otherwise the variant's base price, adjusted by the price list's **percentage** (in
   basis points: `-1000` is 10% off);
3. without a price list, the variant's base price as it is.

Rounding is half away from zero, on whole minor units. Hypha doesn't convert currencies:
a base price counts only in a market of its own currency, so a market in another currency
(GBP beside a EUR catalog) gives its variants fixed prices.

## Taxes

Tax rates live in **tax regions**, one per country: a default rate, and rates for **tax
classes**. A product in a class (books, food, exempt) takes that class's rate where the
region sets one, and the default elsewhere; a country without a region charges no tax.
Each market says whether its prices **include tax**. When they do, a €10.00 price is
€10.00 in every country of the market, and the tax inside it varies.
