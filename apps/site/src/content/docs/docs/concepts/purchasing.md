---
title: Purchasing
description: Suppliers, what each sells you at what price, and the stock you buy from them.
sidebar:
  order: 11
---

Purchasing is where stock comes from: the **suppliers** you buy from, what each sells you,
the **purchase orders** you place with them, the goods you **receive** against them, and
what each unit cost you once it's landed. **Transfers** move stock between your own
locations.

It needs the `purchasing:read` and `purchasing:write` scopes, and `purchasing:send` to
email a supplier. Transfers need the inventory scopes instead.

## How a purchase goes

1. **Set your base currency** (Settings → Store, or the setup wizard). Every cost is kept
   in it: what stock is worth, what each order line cost you.
2. **Add the supplier** and, if you like, its **catalogue**: its code and price for each
   variant it sells you.
3. **Draft a purchase order** to one of your locations. Lines take the catalogue's price;
   add shipping, duties or fees as **extra costs**.
4. **Order it**, or **send it**: the supplier gets it by email, and it's marked ordered.
   What it brings now shows as **incoming** stock at that location.
5. **Receive** the goods as they arrive, in as many parts as they come. Accepted units go
   on hand at their **landed cost**, and each item's cost becomes the average of what it
   had and what arrived.
6. A bill that comes later is added to the order's extra costs: the stock that arrived is
   **revalued**.

Every step leaves a row in each item's [stock history](/docs/concepts/inventory/#stock-history).

## In the admin

**Purchasing** in the side menu has three pages:

- **Purchase orders**: every order with its status and how much arrived, filtered by
  view. An order's page has its lines (with what's still to come and each unit's landed
  cost), its extra costs, totals in its currency and yours, and the actions its status
  allows: *Mark as ordered*, *Send to supplier* (*Send again*, or *Send updated PO* after
  an edit), *Receive*, *Close*, *Cancel*, *Print*. Its receipts are listed below, each
  with *Take back*.
- **Suppliers**: details, contacts and payment terms; a **Catalogue** tab to add, change
  and remove what they sell you; and their purchase orders.
- **Transfers**: moving stock between your locations, with *Ship* and *Receive*.

The **Receive** dialog starts with what's still to come on each line: change it to what
arrived, put damaged or wrong units under *Refused*, and confirm when more came than was
ordered. An inventory item's page shows its cost (*Change cost*), what's incoming at each
location, and its history, each row linking to the order, receipt or transfer behind it.

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

- **lines**, one per variant (up to 1,000, each up to 1,000,000,000 units:
  `422 quantity_too_large`): a quantity and a unit cost in the order's currency, net of
  tax. A line sent without a cost takes the supplier's catalogue price,
  else the cost on the supplier's last order with that variant;
- **extra costs** (up to 20): shipping, duties, fees or other, in the order's currency or
  your store's, spread over the lines **by value** (the default), **by quantity**, or not
  at all (`none`: an expense on the total);
- a tax amount (on the total only: purchase tax is in no unit's cost), an expected date
  (a calendar day, set by default from the supplier's lead time when it's ordered), the
  supplier's own reference, a note printed for the supplier and one for staff.

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
Once a purchase order is ordered, the store's base currency is fixed
(`409 store_currency_locked`): its rate converts into it.
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
  `details.lines`, unless the request says `"allow_over": true`. The extra costs were
  spread over what was ordered, so units past that come in at the line's price alone:
  stock never carries more of a cost than was paid.
- **A variant with no stock record** gets one, **untracked**, so receiving doesn't change
  whether it sells; the answer lists it in `new_items` for you to turn tracking on. A
  variant removed from its product since is still counted on the order and the receipt,
  but no stock is added for it.

The order counts what arrived and closes once every line is accounted for. Its receipts
are at `GET /admin/purchase-orders/{id}/receipts`. A receipt recorded by mistake is
**taken back** with `POST /admin/receipts/{id}/cancel`: its units come off stock (the
count may go below zero if some were sold since), its value comes out of the average
cost, and a closed order opens again. Events `receipt.created` and `receipt.cancelled`
reach apps whose grant holds `purchasing:read`.

### Costs that come late

A carrier's bill or the duties often arrive after the goods. Add or change the order's
extra costs then, even on a **closed** order (whose costs are all that still change),
with the same `PATCH /admin/purchase-orders/{id}`:

- **What arrived is revalued.** Each line's share of the extra costs per unit changes,
  and the difference for the units already received (each receipt at its own exchange
  rate) is spread over what's on hand now: the item's cost moves by it, with a
  `cost_adjustment` row in its history whose source is the order. A lower cost takes
  value back off, never below zero.
- **Units already sold keep their cost.** Only the stock on hand is revalued; with none
  left, nothing moves.
- **Only the difference counts.** The order keeps what its stock carries already, so
  saving the same costs again changes nothing.
- **Closing early** cancels what didn't come, so the costs spread over fewer units and
  the stock that arrived is revalued for its bigger share. Taking a receipt back keeps
  the rest right the same way.

## Incoming stock

What's on its way is read from your open orders, never stored: each level of an item in
the admin API (`GET /admin/inventory-items/{id}`, `/admin/variants/{id}/inventory`,
`/admin/products/{id}/inventory` and the inventory list) carries **`incoming`**, the units
that ordered purchase orders to that location still bring (ordered, less what arrived,
was refused or cancelled), and **`next_expected_at`**, the soonest of those orders'
expected dates. Transfers in transit to the location count too (shipped, less what
arrived or was lost), with their expected dates. A location with units coming but none
yet is listed with zero counts. Drafts, closed and cancelled orders bring nothing.
Incoming units can't be sold, and storefronts don't see them.

## Transfers

A **transfer** moves stock between two of your locations, with a time in transit between
leaving one and reaching the other. It needs `inventory:write` (`inventory:read` to see
one), not the purchasing scopes.

1. **Draft it:** `POST /admin/transfers` with an `origin`, a `destination` and one line per
   variant (`{ "variant": "var_…", "quantity": 4 }`). Each variant needs a stock record:
   there's nothing at the origin to move otherwise (`422 variant_not_stocked`). A draft
   changes freely (`PATCH`) and can be deleted or cancelled. Transfers are numbered
   `TR-1001` and up.
2. **Ship it:** `POST /admin/transfers/{id}/ship` sends every line whole. The units leave
   the origin now (a `transfer_out` row in each item's history; the count may go below
   zero, as fulfilling can) and are **in transit** until received, showing as incoming at
   the destination.
3. **Receive it,** in as many parts as the goods arrive:
   `POST /admin/transfers/{id}/receive` with how many of each variant were **accepted**
   (on hand at the destination, a `transfer_in` row) and how many were **lost** on the
   way (`rejected`: a `shrinkage` row at the destination with no units, so the history
   still says where they went). No more than a line has in transit
   (`409 over_received`, with each such line's `in_transit` in `details.lines`). Once
   every unit is accounted for, the transfer is `received`.

Shipping and receiving take an `Idempotency-Key`: a retry answers with the transfer
without moving anything again. A transfer changes no cost: an item's unit cost is the
same wherever its units are. A shipped transfer can't be taken back; bring the stock
back with another transfer. Events `transfer.created`, `.updated`, `.shipped`,
`.received` (with the lines that arrived) and `.cancelled` reach apps whose grant holds
`inventory:read`.

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
- It sets `sent_at` and `sent_revision` (the revision that went out), and records
  `purchase_order.sent`. The emails are listed with
  `GET /admin/notifications?purchase_order={id}`, and can be resent from there, which
  needs the same scopes as sending.
- **Sending needs `purchasing:send`** as well as `purchasing:write`. An agent may draft
  purchase orders, but only proposes sending one: a person approves every email to a
  supplier.

The admin's purchase order page also has a print view, which the browser can save as a
PDF. Extra costs paid in your own currency (duties at home) are left off what the supplier
sees, in the email and the print view alike.
