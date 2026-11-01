---
title: Purchasing
description: Suppliers, what each sells you at what price, and the stock you buy from them.
sidebar:
  order: 11
---

Purchasing is where stock comes from: the **suppliers** you buy from, what each sells you,
the **purchase orders** you place with them, and (as it lands) the goods you receive. It
needs the `purchasing:read` and `purchasing:write` scopes.

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

## Purchase orders

A purchase order (`po_…`, numbered `PO-1001`, `POST /admin/purchase-orders`) is what you
order from one supplier, to one of your locations (its **destination**). It starts as a
**draft** in the supplier's currency with their payment terms, and has:

- **lines**, one per variant (up to 1,000): a quantity and a unit cost in the order's
  currency, net of tax. A line sent without a cost takes the supplier's catalogue price,
  else the cost on the supplier's last order with that variant;
- **extra costs** (up to 20): shipping, duties, fees or other, in the order's currency or
  your store's, spread over the lines **by value** (the default), **by quantity**, or not
  at all (`none`: an expense on the total);
- a tax amount (on the total only: purchase tax is in no unit's cost), an expected date,
  the supplier's own reference, a note printed for the supplier and one for staff.

Each line shows its **landed unit cost**: its price plus its share of the extra costs, in
your store's base currency. An order in another currency needs an **exchange rate** (one
unit of it in yours, as a decimal string like `"0.9213"`) before it's ordered.

### Statuses

| Status | Means | Next |
| --- | --- | --- |
| `draft` | Being written: anything changes, and it can be deleted | `POST …/order`, `…/cancel` |
| `ordered` | Placed with the supplier: goods may arrive | `…/close`, `…/cancel` (nothing arrived yet) |
| `closed` | Done: everything arrived, or what didn't was cancelled | — |
| `cancelled` | Called off before anything arrived | — |

Ordering needs a destination, a line, your store's base currency (`409
store_currency_required`) and, in another currency, a rate (`409 exchange_rate_required`).
An ordered order still changes (`PATCH`): lines can be added and their quantity or cost
changed, but a line goods arrived on stays, at no less than arrived (`422 below_received`),
and the currency is fixed. Each change counts a **revision**. **Closing** an ordered one
cancels what's still outstanding.

`receipt_status` says how much arrived: `none`, `partial` or `received` (every line
accounted for). An order closes by itself once its last line is.

`GET /admin/purchase-orders` lists them by `supplier`, `status`, `receipt_status`,
`destination` or `variant`, newest first, oldest first (`sort=number_asc`) or soonest due
first (`sort=expected_at`).

Purchase order events (`purchase_order.created`, `.updated` with the fields and the
revision, `.ordered`, `.closed`, `.cancelled`, `.receipt_status_changed`) reach apps whose
grant holds `purchasing:read`.

## Receiving

When goods arrive, record a **receipt** against the ordered purchase order:
`POST /admin/purchase-orders/{id}/receipts` with how many of each variant you **accepted**
and how many you **refused** (damaged, wrong), at the order's destination unless you name
another `location`. Send an `Idempotency-Key`: a retry answers with the first receipt.

```json
{ "lines": [{ "variant": "var_…", "accepted": 5, "rejected": 1 }], "note": "Pallet 1 of 2" }
```

- **Accepted units go on hand** at the location, each at its **landed cost**: the line's
  unit cost plus its share of the extra costs, at the order's exchange rate, in your
  base currency. A `received` row joins the item's stock history.
- **The item's unit cost becomes the moving average** of what was on hand (across every
  location) and what arrived: 4 at €5.00 and 6 arriving at €10.00 make €8.00.
- **Refused units add no stock**, but they're counted: a line is done once received and
  refused units cover it. To expect a replacement, raise the line's quantity.
- **More than is outstanding** is `409 over_received`, listing each line's outstanding in
  `details.lines`, unless the request says `"allow_over": true`.
- **A variant with no stock record** gets one, **untracked**, so receiving doesn't change
  whether it sells; the answer lists it in `new_items` for you to turn tracking on.

The order counts what arrived and closes once every line is accounted for. Its receipts
are at `GET /admin/purchase-orders/{id}/receipts`. A receipt recorded by mistake is
**taken back** with `POST /admin/receipts/{id}/cancel`: its units come off stock (the
count may go below zero if some were sold since), its value comes out of the average
cost, and a closed order opens again. Events `receipt.created` and `receipt.cancelled`
reach apps whose grant holds `purchasing:read`.

## Incoming stock

What's on its way is read from your open orders, never stored: each level of an item in
the admin API (`GET /admin/inventory-items/{id}`, `/admin/variants/{id}/inventory`,
`/admin/products/{id}/inventory` and the inventory list) carries **`incoming`**, the units
that ordered purchase orders to that location still bring (ordered, less what arrived,
was refused or cancelled), and **`next_expected_at`**, the soonest of those orders'
expected dates. A location with units coming but none yet is listed with zero counts.
Drafts, closed and cancelled orders bring nothing. Incoming units can't be sold, and
storefronts don't see them.

## Sending a purchase order

`POST /admin/purchase-orders/{id}/send` emails the order to the supplier, one email per
address, through the store's email settings like every other email (the editable
**Purchase order** template, `supplier_purchase_order`):

```json
{ "cc": ["accounts@supplier.example"], "message": "Before the spring sale, please." }
```

- **`to`** defaults to the supplier's primary contact; a supplier without an email needs
  the addresses named (`422 supplier_email_required`). At most 10 addresses in all.
- **A draft is ordered as it's sent**, by the same rules as `…/order`. An ordered one is
  sent again as it is now, with its revision: after editing an order, send it again.
- The email lists the lines (with the supplier's own codes), the extra costs in the
  order's currency, the totals, the payment terms, where to ship and your note.
- It sets `sent_at` and records `purchase_order.sent`. The emails are listed with
  `GET /admin/notifications?purchase_order={id}`, and can be resent from there.
- **Sending needs `purchasing:send`** as well as `purchasing:write`. An agent may draft
  purchase orders, but only proposes sending one: a person approves every email to a
  supplier.

The admin's purchase order page also has a print view, which the browser can save as a
PDF.
