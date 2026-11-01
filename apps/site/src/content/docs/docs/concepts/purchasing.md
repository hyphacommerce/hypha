---
title: Purchasing
description: Suppliers, what each sells you at what price, and the stock you buy from them.
sidebar:
  order: 11
---

Purchasing is where stock comes from: the **suppliers** you buy from, what each sells you,
and (as it lands) the purchase orders you send them and the goods you receive. It needs
the `purchasing:read` and `purchasing:write` scopes.

## Suppliers

A supplier (`sup_…`, `POST /admin/suppliers`) has a name and a **currency**, the one
they invoice in. Beside those:

- **contacts**: up to 10, each with a name, email and phone; one may be the **primary**
  contact, where purchase orders are emailed;
- **payment terms**: `in_advance`, `on_receipt`, or `net` with its days (1 to 365), each
  with an optional note (`{"type": "net", "days": 30, "note": "2% off within 10 days"}`);
- a **lead time** in days, the default for when an order arrives;
- an address, a tax number, and notes for staff.

Change one with `PATCH /admin/suppliers/{id}` (JSON Merge Patch). A supplier you no longer
buy from is **archived** (`"archived": true`): it takes no new purchase orders. Deleting
one takes its catalogue with it; once purchase orders name it, it can only be archived
(`409 supplier_in_use`).

Supplier events (`supplier.created`, `supplier.updated` with the fields that changed,
`supplier.deleted`) reach apps whose grant holds `purchasing:read`.

## Catalogues

What a supplier sells is a row per variant (`PUT /admin/suppliers/{id}/variants/{variant_id}`):

- their own code for it (`supplier_sku`), printed on purchase orders;
- their price per unit (`unit_cost`), in the supplier's currency, to 4 decimals past the
  minor unit like every cost;
- the least they sell (`min_order_quantity`) and a lead time that overrides the
  supplier's.

`GET /admin/suppliers/{id}/variants` lists a supplier's catalogue, and
`GET /admin/variants/{id}/suppliers` who sells a variant and at what. A supplier's currency
stays fixed while its catalogue has prices (`409 supplier_currency_in_use`), so a price is
never read in the wrong currency. A variant removed from its product leaves every
catalogue.
