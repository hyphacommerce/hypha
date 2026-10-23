---
title: Products, variants and custom fields
description: Options, sparse variants, categories, media and the custom fields around them.
sidebar:
  order: 4
---

Hypha's catalog is built for products that aren't simple: options that combine only in
some ways, data of your own on every record, and apps that keep fields of their own.

## Products and variants

A **product** has a handle (its URL name), a title, a description in rich text, and a
status: `draft`, `active` or `archived`. Its **options** are the choices a shopper makes
(frame size, drivetrain, finish), each with its values.

A **variant** is one combination of option values that you actually sell, with its own
base price, SKU, weight and whether it ships. Variants are **sparse and explicit**: you
create the combinations that exist, never the full grid. A bike with 4 frame sizes and 3
drivetrains, sold in 8 combinations, has 8 variants, not 12.

## Categories

Categories form a tree, as deep as the store allows (a setting), with products in as many
categories as you like. Each has a handle, a description and an image.

## Media

Images and files are uploaded straight to the store's file storage (S3-compatible, or R2
on Cloudflare) and attached to products and their variants. A file in use can't be
deleted.

## Custom fields

Custom fields add your own data to **products, variants, categories, customers and
orders**. Each field is defined once, with a type and rules, and every value is checked
against them.

| Kind | Types |
| --- | --- |
| Text | single line, multiline, rich text, URL |
| Numbers | integer, decimal, money, measurement (with a unit) |
| Time | date, date and time |
| Other | boolean, choice from a list, a reference to another record, JSON |

Most types can also be lists. Fields live in **namespaces**: the merchant's own
(`custom.*`), and one for each app (`app_<name>.*`) and spore, which only that app or
spore can write unless it lets staff edit them. Each field says whether storefronts may
read it, and fields holding personal data about customers need a personal-data scope to
read.

Products can be listed, filtered and searched by their fields, and changing a record's
fields records one event, like any other change.
