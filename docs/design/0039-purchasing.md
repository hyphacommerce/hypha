# 0039: Purchasing: suppliers, purchase orders, receiving, transfers, stock history and cost

- **Status:** Built (2026-11-01; "As built" at the end). Accepted 2026-10-31: Danilo
  accepted every recommendation of the research (§1), which became PU1–PU16. §14's
  details and §15's edges were confirmed on 2026-11-01, with unit costs sent as decimal
  strings of minor units.
- **Scope:**
  - Suppliers, and each supplier's codes and prices for the variants it sells.
  - Purchase orders: drafting, ordering, sending to the supplier, editing, closing,
    cancelling.
  - Receiving goods into stock at a location, in several receipts, with accepted and
    rejected units.
  - Transfers between the store's locations, with a time in transit.
  - A ledger of every change to stock, with a reason and what caused it.
  - Unit cost: kept per item as a moving average, landed costs spread over a PO's lines,
    and a base currency for the store to keep it in.
  - Incoming stock, derived from open POs and transfers.
- **Not in scope:**
  - Reorder points and suggested reorders.
  - Supplier portals.
  - Bills, payments to suppliers, 3-way matching.
  - Accounting sync (a QuickBooks/Xero spore).
  - FIFO layers, lots, serials and expiry; bins.
  - Multi-step receiving with quality control.
  - Returns to suppliers; PO approvals.
  - Purchase units and pack sizes; price breaks per supplier.
  - `damaged`/`quality_control` quantity states.
  - Several shipments per transfer; holding stock for a transfer before it ships.
  - Dropship.
  - PDFs of a PO (§10). Custom fields on suppliers and POs (0033).
  - Storefront use of incoming stock.
  - These are §13's list.
- **Related:**
  - 0004 (inventory: items, levels, reservations, I7 and §7's "later": incoming,
    reasons, history).
  - 0018 (fulfilment: a record per event, counters on the parent's lines, one workflow
    batch retried on a lost race; this note repeats the shape).
  - 0001 (versions, batches), 0016 (events, the outbox: the ledger is written the same
    way).
  - 0021 (store setup: the base currency joins it).
  - 0028 (notifications: the PO email).
  - 0029 (scopes, apps' event routing), 0035 (agents: proposals).
  - 0037 (price lists: margins over cost come later through analytics).

## Why

Merchants who hold stock buy it from suppliers. Today Hypha only lets them type a new
count (`set_on_hand`) or add a delta (`adjust_on_hand`). Neither leaves a trace, a reason
or a cost. That gives four gaps:

1. **No record of what was ordered, from whom, at what price,** and what is still to
   come.
2. **No cost.** Without it there's no markup to set, no margin, no stock value. Analytics
   needs a cost it can trust.
3. **No history.** "Why is this 4 and not 12?" can't be answered; 0004 §7 deferred
   adjustment reasons and history.
4. **No way to move stock between locations** except two hand-edits that don't know
   about each other.

## Decisions

| | Decision |
| --- | --- |
| PU1 | **A new leaf, `hypha-purchasing`** (core only): `Supplier` (`sup_…`), the supplier catalogue, `PurchaseOrder` (`po_…`) and `Receipt` (`rcpt_…`), with their events and repositories (§2–§5) |
| PU2 | **A new workflow, `hypha-receiving`**: `receive` and `cancel_receipt`. Each writes the PO, the receipt and the inventory items (with their ledger rows) in **one batch**, retried on a lost race like `fulfill` (§5) |
| PU3 | **POs and transfers are separate aggregates**, not Shopify's "a PO is received through a transfer" nor Lightspeed X's one typed object. **`Transfer` (`trf_…`) lives in `hypha-inventory`.** They share one receive step: accepting and rejecting counts, writing stock and the ledger (§6) |
| PU4 | **A receipt is a record per arrival** (where, when, accepted and rejected per line, the cost and rate applied), **and the PO's lines keep counters.** The PO is the unit of consistency, as the order is for fulfilments (0018 FU3) (§5) |
| PU5 | **Stock history is an append-only ledger, `stock_movements`, recorded by the `InventoryItem` itself** and written by the adapter in the item's transaction, as events are. No write can change stock without a row. Each row has a typed reason and source, the unit cost at that moment, the actor and the quantity after. An opening row per existing level makes the sums hold from the start (§7) |
| PU6 | **Unit cost is a moving (weighted) average per item, across all locations,** in the store's base currency. It changes only on stock coming in, a late cost or a hand edit. Every ledger row records the cost, so cost of goods sold and stock value never move when cost changes later, and FIFO stays computable from the ledger (§8) |
| PU7 | **The store gets a base currency.** Every cost is kept in it. It is set once, and stays changeable only while no cost has been recorded (§8.4) |
| PU8 | **Incoming is derived**, never a stored counter: what open POs and in-transit transfers still bring, per variant and location, with the expected date. Storefronts don't use it in v1 (§9) |
| PU9 | **A supplier's catalogue is rows of their own,** keyed by (supplier, variant): the supplier's SKU, unit cost, minimum order quantity, lead time. They are not children of the supplier, which may list thousands (§3) |
| PU10 | **PO statuses:** `status` is `draft` → `ordered` → `closed`, or `cancelled`. `receipt_status` (`none`/`partial`/`received`) is derived from the lines and stored. Closing early cancels what's outstanding. "Sent" is a timestamp, not a status (§4) |
| PU11 | **Receiving more than is outstanding is refused** (`409 over_received`) unless the request says `allow_over`. **Rejected units are recorded and add no stock** (§5) |
| PU12 | **Extra costs** (shipping, duties, fees, other) are rows on the PO, spread over its lines by value (default), by quantity, or not at all (an expense). **A cost changed after a receipt** revalues the units still on hand (§4.3, §8.3) |
| PU13 | **New scopes `purchasing:read` and `purchasing:write`** for suppliers, POs, receiving and costs; **`purchasing:send`** to email a supplier, which agents may only propose. Transfers need `inventory:write`. **Cost is shown only to callers holding `purchasing:read`**, staff sessions included (§11) |
| PU14 | **Order lines keep `unit_cost`**, copied at checkout from the item's average, so margin per order doesn't move before fulfilment (§8.5) |
| PU15 | **Sending a PO is an email** through hypha-mail (a new `supplier_purchase_order` template, queued by the `Mailer`), **plus a print view in the admin.** No PDF or public link yet (§10) |
| PU16 | **Adjustment reasons are a closed enum plus a note.** `adjust_on_hand` gains an optional `reason` (default `correction`); `set_on_hand` records `counted` (§7.2) |

## 1. What others do

The research behind this note (2026-10-31) covered Shopify, Medusa v2, Saleor,
commercetools, Sylius, Vendure, Odoo, Zoho Inventory, inFlow, Cin7 Core, Katana and
Lightspeed Retail (X- and R-Series). The main findings:

**Purchasing among commerce platforms**
- Only Shopify has it, and it is new and thin.
  - POs are `DRAFT` → `ORDERED`, then archived.
  - Supplier currency and payment terms live on each PO.
  - A supplier's SKU and cost are copied from the previous PO; there is no catalogue.
  - Sending is a PDF export: "You can't email a purchase order from Shopify".
  - The PO API has been read-only since 2026-10.
  - Sources: [creating POs](https://help.shopify.com/en/manual/inventory-and-locations/purchase-orders/creating-purchase-orders), [Stocky transition](https://help.shopify.com/en/manual/products/inventory/transitioning-from-stocky), [PO read API](https://shopify.dev/changelog/posts/purchase-order-read-api).
- Medusa, Saleor, commercetools, Sylius and Vendure have stock per location and nothing
  for purchasing.

**How POs are received**
- **Shopify: through a transfer.** A PO links to an `InventoryTransfer` whose origin is
  the supplier. Arrivals are `InventoryShipment`s whose lines count accepted, rejected
  and canceled units. Once linked, the transfer "operates independently"; changes don't
  sync with the PO ([PO transfers](https://help.shopify.com/en/manual/inventory-and-locations/purchase-orders/creating-inventory-transfers), [`InventoryShipment`](https://shopify.dev/docs/api/admin-graphql/latest/objects/inventoryshipment)).
- **A record per arrival elsewhere:** Cin7's stock-received tasks, Zoho's Purchase
  Receives, Odoo's pickings with backorders, Lightspeed X's "deliveries".
  - Lightspeed X added deliveries in 2025 because counters on the lines couldn't track
    several arrivals.
- **Counters only:** Lightspeed R (`numReceived`/`checkedIn`).

**Transfers**
- Separate objects with an in-transit state almost everywhere: Cin7, Katana, Lightspeed
  R, Zoho, inFlow.
- Stock leaves the origin when the goods leave and reaches the destination when they
  arrive.
- Odoo uses one generic move document for everything, with virtual transit and
  adjustment locations.

**Stock history**
- **Shopify** has the richest ledger ([`InventoryAdjustmentGroup`](https://shopify.dev/docs/api/admin-graphql/latest/objects/InventoryAdjustmentGroup), [`InventoryChangeInput`](https://shopify.dev/docs/api/admin-graphql/latest/input-objects/InventoryChangeInput)):
  - per event: a reason, a URI for the document that caused it, staff or app;
  - per change: `delta` and `quantityAfterChange`.
- **Katana** ([inventory movements](https://developer.katanamrp.com/reference/list-all-inventory-movements)) adds `value_per_unit` and `average_cost_after`.
- **Lightspeed R** ([`InventoryLog`](https://developers.lightspeedhq.com/retail/endpoints/InventoryLog/)) links each row to its sale, order, transfer or vendor return.

**Incoming stock**
- **Stored** in Shopify (filled by transfers, not POs), Katana, Cin7 and Lightspeed R.
- **Derived** in Odoo (`incoming_qty` from open moves, [source](https://raw.githubusercontent.com/odoo/odoo/18.0/addons/stock/models/product.py)) and Lightspeed X.
- Nobody sells against it.

**Cost**
- **Shopify:** one static `unitCost` that receiving doesn't change.
- **Moving or weighted average:** Katana (per location), Lightspeed X (per outlet),
  Lightspeed R (account-wide), inFlow (the default), Zoho, Odoo AVCO.
- **FIFO:** Cin7, Odoo, Lightspeed R, inFlow, Zoho.
- IAS 2 permits FIFO and weighted average and forbids LIFO ([IAS 2](https://www.ifrs.org/issued-standards/list-of-standards/ias-2-inventories/)).
- **Extra costs** are spread into unit cost by value or quantity everywhere that has
  them (Katana `BY_VALUE`/`NON_DISTRIBUTED`, inFlow by price, weight or volume, Odoo's
  landed-cost document). Odoo applies a late cost only to the units still in stock.

**What we take**
- Odoo's supplier catalogue rows and its rule for editing a confirmed PO.
- Separate receipt records with counters on the lines (Cin7, Zoho; our fulfilment).
- Accept and reject per line (Shopify).
- Separate transfers with in transit (most tools).
- A ledger with reason, source, quantity after and cost (Shopify, Katana).
- Incoming derived (Odoo).
- Moving average (Katana, inFlow).
- Landed cost by value or quantity (Katana).
- Late costs on units still on hand (Odoo).

**What we don't take**
- Shopify's PO-through-a-transfer link, Lightspeed X's one-object model, Odoo's generic
  moves.
- Stored incoming counters: they would race checkouts on the item's version, which is
  our overselling lock (0004 I7).
- FIFO now.
- Cost per location.

## 2. The model at a glance

```
hypha-purchasing (leaf)                         hypha-inventory (leaf)
  Supplier ──< SupplierVariant (rows)             Location
  PurchaseOrder                                   InventoryItem ── levels, reservations,
    lines (by variant): quantity, unit_cost,        unit_cost (moving average),
      received, rejected, cancelled                  records StockMovements ──> stock_movements
    costs: shipping | duties | fees | other       Transfer
  Receipt (per arrival, per location)               lines (by variant): quantity,
                                                      shipped, received, rejected
hypha-receiving (workflow)
  receive / cancel_receipt: PO + Receipt + items, one batch
```

New core IDs:
- `SupplierId` (`sup_`), `PurchaseOrderId` (`po_`), `ReceiptId` (`rcpt_`), `TransferId`
  (`trf_`).
- Ledger rows carry no prefixed ID. They are ordered by a `bigint` position, like
  `outbox_events`.
- A PO or transfer line is keyed by its variant (one line per variant per document), so
  lines need no IDs.

## 3. Suppliers (PU9)

**`Supplier`** (aggregate, versioned, stamped):
- `name` (required).
- `contacts`: at most 10, each `{ name, email, phone, primary }`; at most one primary.
  The primary's email is where POs go, the others are cc'd when chosen.
- `address`: core's `Address`.
- `currency`: the default for its POs.
- `payment_terms: Option<PaymentTerms>`, and `lead_time_days: Option<u32>` (the default
  for a PO's expected date).
- `tax_number`, `notes`.
- `archived: bool`.
  - Archived suppliers can't get new POs.
  - A supplier is deleted only while no PO names it (`409 supplier_in_use`), so POs keep
    their supplier link.

**`PaymentTerms`:**
- One of `InAdvance`, `OnReceipt`, `Net { days }` (1–365), with an optional note.
- Typed rather than free text (Shopify's) so a later accounting spore and agents can act
  on it.
- The due date is shown, computed from the PO's order date or its first receipt.

**`SupplierVariant`** (rows keyed by (supplier, variant); `keys::SUPPLIER_VARIANT`):
- `supplier_sku`.
- `unit_cost: Option<UnitCost>` in the supplier's currency.
- `min_order_quantity: Option<u32>`, `lead_time_days: Option<u32>` (overrides the
  supplier's).
- Set with `PUT`, removed with `DELETE`, listed per supplier and per variant.
- Changing a supplier's currency is refused while its rows have costs
  (`409 supplier_currency_in_use`), so a row's cost is never read in the wrong currency.
- A variant removed later leaves its rows. `VariantCleanup` in `hypha-dispatch` (on
  `product.variant_removed` and `product.deleted`) deletes them too, as it does the
  variant's fixed prices.

**Pre-filling a PO line:** the (supplier, variant) row's SKU and cost, else the last PO
line for that variant from the same supplier (Shopify), else the variant's SKU and no
cost.

## 4. Purchase orders (PU10, PU12)

### 4.1 The aggregate

`PurchaseOrder`:
- `number`: `PurchaseOrderNumber`, shown `PO-1001`, from the sequence
  `purchase_order_numbers` (atomic and never rolled back, like orders' numbers). A
  configurable prefix is later.
- `supplier`.
- `supplier_name`: a snapshot taken when the PO is ordered, for documents.
- `destination: LocationId`: optional on a draft, required to order.
- `currency`, and `exchange_rate: Option<ExchangeRate>` (one unit of the PO's currency
  in the store's).
  - Required to order when the currency isn't the store's (`exchange_rate_required`);
    absent when it is.
  - Editable while ordered: each receipt snapshots the rate it used (§5).
- `payment_terms` (copied from the supplier, editable).
- `expected_at: Option<Date>`: ordered date + lead time by default, editable.
- `supplier_reference` (their order or confirmation number), `note_to_supplier`
  (printed and emailed), `note` (internal).
- `lines`: 1 to `MAX_LINES` (1,000), one per variant. Each line:
  - `variant`
  - snapshots of `product_title`, `variant_title`, `sku` (variants may be removed later)
  - `supplier_sku`
  - `quantity` (≥ 1), `unit_cost: UnitCost` (in the PO's currency, net of tax)
  - `received`, `rejected`, `cancelled` (u32 counters)
- `costs`: at most 20, each:
  - `{ kind: shipping | duties | fees | other, description, amount: Money, allocation:
    by_value | by_quantity | none }`
  - `amount` is in the PO's currency or the store's: duties paid at home often are.
- `tax: Option<Money>`: for the PO's total only. It never enters cost (purchase VAT is
  usually recovered).
- `status`, `receipt_status`, `ordered_at`, `sent_at` (last sent), `closed_at`,
  `cancelled_at`.
- `revision`: bumped by each edit while ordered.

**A line's arithmetic:**
- `outstanding = quantity − received − rejected − cancelled`, floored at 0.
- A line is accounted for when `outstanding` is 0.
- Rejected units count as accounted: a supplier's replacement is expected by raising the
  line's quantity.

### 4.2 Statuses and moves

| From | Action | To | Rule |
| --- | --- | --- | --- |
| — | create | `draft` | Supplier not archived |
| `draft` | edit anything | `draft` | |
| `draft` | delete | — | Only drafts are deleted |
| `draft` | `order` | `ordered` | Destination set; rate set when needed; at least one line |
| `draft`, `ordered` | `send` | `ordered` | Emails the supplier (§10); orders a draft first; sets `sent_at` |
| `ordered` | edit (§4.4) | `ordered` | `revision` + 1 |
| `ordered` | `close` | `closed` | Every line's `outstanding` moves to `cancelled` |
| `draft`, `ordered` | `cancel` | `cancelled` | Refused once anything is received (`purchase_order_received`: close it instead) |
| `ordered` | last line accounted for (a receipt) | `closed` | Automatic |
| `closed` | `cancel_receipt` (§5) | `ordered` | A closed PO whose receipt is taken back reopens |

- `receipt_status` is recomputed whenever a counter changes:
  - `none`: nothing received or rejected;
  - `received`: every line accounted for by receipts, not by closing;
  - `partial` otherwise.
- Closed and cancelled POs are read-only, but cost rows stay editable on a closed PO
  (§8.3).

### 4.3 Extra costs and landed cost

Allocation is computed over the PO's **ordered** quantities, so a partial receipt gets
the right per-unit share without waiting for the rest:

- **`by_value`:** a line's share of a cost is `cost × line value ÷ Σ line values`, where
  a line's value is `quantity × unit_cost` and cancelled quantities are left out.
- **`by_quantity`:** the same with quantities.
- **`none`:** the cost is on the PO's total but in no unit cost (an expense).
- A line's **extra per unit** is the sum of its shares ÷ its quantity.

The **landed unit cost** of a received unit, in the store's currency:

    landed = (unit_cost + Σ extra per unit in the PO's currency) × rate
           + Σ extra per unit in the store's currency

`rate` is the one snapshotted by the receipt. Rounding is half away from zero, with
`i128` intermediates, at `UnitCost`'s precision (§14).

### 4.4 Editing an ordered PO

This is Odoo's rule:
- Lines can be added, and quantities and unit costs changed.
- A line's quantity can't drop below `received + rejected` (`422 below_received`).
- A line with any receipt can't be removed.
- Costs, terms, the rate, notes, `expected_at` and the destination stay editable. A new
  destination applies to later receipts.
- Each edit records `purchase_order.updated` (what changed) and bumps `revision`. The
  admin then offers "Send updated PO", which emails the new revision.

## 5. Receiving (PU2, PU4, PU11)

**`Receipt`** (aggregate):
- `purchase_order`, `location` (the PO's destination unless the request names another).
- `lines`: `{ variant, accepted, rejected, landed_unit_cost: Option<UnitCost> }`, at
  least one, each with `accepted + rejected ≥ 1`.
- `exchange_rate`: the rate used, if any.
- `note`.
- `status`: `Recorded` | `Cancelled { at }`.
- `created_at` is when it was received. Back-dating is later.

**`receive`** (`purchasing:write`), in a retry loop like `fulfill`:
1. Require the scope; replay if the `Idempotency-Key` was used.
2. Load the PO. Refuse it unless `ordered` (`purchase_order_not_ordered`, 409).
3. Check the lines.
   - Unknown variants are `unknown_purchase_order_line` (422).
   - More than a line's `outstanding` without `allow_over` is `over_received` (409),
     with each line's outstanding in `details`.
4. Load the location and the lines' inventory items. A variant without an item gets one
   **untracked**, so receiving never changes whether a variant sells. The response flags
   it, and the admin offers to turn tracking on.
5. For each line:
   - `item.receive(location, accepted, landed, source)`: on hand up, the moving average
     (§8.2), a `received` ledger row.
   - Rejected units change no stock.
6. Update the PO's counters, its `receipt_status` and possibly `status`.
7. Stage the receipt (insert), the PO (update), the items (update; insert for new ones)
   and the idempotency record, then commit.
8. If a race was lost (`StaleVersion`), retry from step 2 on fresh data. A concurrent
   receipt then shows as `over_received`, not as a conflict.

A receipt touching many items can lose races to checkouts on those items. It retries
like any other write. A receipt of hundreds of lines during a sale may need more attempts
than `MAX_ATTEMPTS`; that's measured in the Postgres race test.

**`cancel_receipt`** (`purchasing:write`) mirrors `cancel_fulfillment`, in one batch:
- The receipt is `Cancelled`.
- The PO's counters go back, and a closed PO reopens.
- Each item gets `unreceive(location, accepted, landed, source)`:
  - on hand down (it may go negative, as fulfilment's can);
  - the receipt's value comes out of the average (§8.2);
  - a ledger row with reason `received` and a negative delta, source the receipt.
- Refused on a cancelled PO.

## 6. Transfers (PU3)

**`Transfer`** (aggregate in `hypha-inventory`):
- `number`: `TR-1001`, from its own sequence.
- `origin` and `destination` locations, which must differ.
- `lines`: one per variant, 1 to 1,000, each `{ variant, quantity, shipped, received,
  rejected }`.
- `expected_at`, `reference` (a carrier or tracking note, free text), `note`.
- `status`: `draft` → `in_transit` → `received`, or `cancelled` from `draft`.
- `shipped_at`, `received_at`.

**Actions** (`inventory:write`, each in one batch with the items, retried like the
others, with `Idempotency-Key`):
- **`ship`:** every line ships whole (`shipped = quantity`).
  - Each item: `transfer_out(origin, quantity, source)`, on hand down at the origin.
  - The stock may go negative there, which the admin warns about first, as fulfilling
    can.
- **`receive`** (in parts):
  - `{ variant, accepted, rejected }` per line, capped at what's shipped and not yet
    accounted for (`over_received`, no `allow_over`: the units are ours).
  - Accepted units: `transfer_in(destination, accepted, source)`.
  - Rejected units: lost in transit, recorded as a `shrinkage` row at the destination
    with delta 0. The origin already gave them up, so the ledger still says where they
    went.
  - The transfer is `received` once every line is accounted for.
- **`cancel`:** drafts only. Taking back a shipped transfer is receiving it back at the
  origin: a later "return to origin" action, not v1.

Transfers change no cost: the average is per item (PU6). Items without an item record
can't be transferred (`variant_not_stocked`). There's nothing at the origin to move.

## 7. The stock ledger (PU5, PU16)

### 7.1 How it's written

- **The item records its movements.** Every method that changes `on_hand` or the cost
  takes a cause and records a `StockMovement`, as an aggregate records events
  (`Recorded`).
- **The adapter writes them** in the same transaction as the item, on every path:
  - `InventoryItemRepo::insert`, `update`, `update_many`, `delete`;
  - `StageInventoryItems`.
- This is the same mechanism that writes `outbox_events`, so no path that changes stock
  can skip the ledger. The repository contract gains a `stock_movements` scenario: after
  any sequence of writes, the sum of an item's deltas at a location equals its
  `on_hand`.
- **The actor** comes from the ports' `ActingAs`, as for events.

**A row (`stock_movements`):**
- `position` (bigint, the order of commits), `item`, `variant`, `location` (none for a
  cost change).
- `quantity`: which counter. Only `on_hand` in v1; the column leaves room for
  `damaged` and the like.
- `delta`, `after`.
- `reason`, `source` (below).
- `unit_cost`: the cost of these units. The landed cost for `received`, the average at
  that moment for anything else, none while the item has no cost.
- `average_after`.
- `actor`, `note`, `at`.

### 7.2 Reasons and sources

**`reason`** (a closed enum, additive):

| Reason | Written by |
| --- | --- |
| `received` | `receive` (positive), `cancel_receipt` (negative) |
| `transfer_out`, `transfer_in` | transfers |
| `sold` | `fulfill` (negative) |
| `restocked` | `cancel_fulfillment`; returns later |
| `counted` | `set_on_hand` (delta = new − old) |
| `correction` | `adjust_on_hand` by default |
| `damaged`, `shrinkage` (theft, loss, lost in transit), `promotion` (samples, gifts), `other` | `adjust_on_hand` with that reason |
| `cost_adjustment` | a cost set by hand, or a late cost (§8.3); delta 0, no location |
| `opening` | the migration's first row per existing level |

**`source`** (a typed enum, not Shopify's URIs, since ours can be linked):
- `Manual`
- `Fulfillment { order, fulfillment }`
- `Receipt { purchase_order, receipt }`
- `PurchaseOrder(id)` (late costs)
- `Transfer(id)`
- `Migration`

All of these are core IDs, so inventory gains no crate edge.

**`adjust_on_hand`** gains `reason` (any of the adjustment reasons; default
`correction`) and `note` (at most 500 characters). Both are optional in the request, so
the route doesn't break. `set_on_hand` gains `note`.

### 7.3 What existing writes record

| Write | Ledger |
| --- | --- |
| Checkout's reservation, `release_stock`, `cancel_order` | Nothing: reservations aren't `on_hand`, and they're traceable per order line already |
| `fulfill` | `sold`, −n at the location the units left, `Fulfillment { order, fulfillment }` |
| `cancel_fulfillment` | `restocked`, +n where they're put back |
| `set_on_hand`, `adjust_on_hand` | `counted`; the given reason |
| Deleting an item | Nothing: its rows stay and name the variant |

An untracked item's `fulfil` changes no counts, so it writes no row; `restock` and
`receive` do, because the count is kept right for when tracking is turned on.

### 7.4 Reading it

- `GET /admin/inventory-items/{id}/movements`: newest first, by `position` cursor,
  filtered by `location`, `reason`, `from`/`to`.
- `unit_cost` and `average_after` are present only with `purchasing:read` (§11).
- The rows are kept forever: they are the source for stock value and cost of goods sold.
  A `jobs.prune` setting can come if anyone needs it.

## 8. Cost (PU6, PU7, PU12, PU14)

### 8.1 Where it lives

- `InventoryItem` gains `unit_cost: Option<UnitCost>` in the store's currency: the
  moving average.
- Merchants who never use POs set it by hand (`PUT /admin/inventory-items/{id}/cost`,
  `purchasing:write`, recorded as `cost_adjustment`), which is Shopify's whole model.
- `InventoryItemChanged` doesn't carry cost. Its readers (search, apps with
  `inventory:read`) shouldn't see it.
- A new `inventory_item.cost_changed` event (item, variant, cost) is routed only to apps
  whose grant has `purchasing:read`.

### 8.2 The moving average

`Q` is the item's `on_hand` summed over every location before the change, and `A` is its
cost.

- **Receiving `q` at `c`:**
  - `A' = (max(Q,0)·A + q·c) / (max(Q,0) + q)`
  - With no `A` yet, or `Q ≤ 0`, `A' = c`.
- **Taking a receipt back (`q` at `c`):**
  - `A' = (Q·A − q·c) / (Q − q)` when `Q − q > 0` and the numerator is positive.
  - Otherwise `A` stays.
- **Everything else** (sales, counts, adjustments, transfers) leaves `A` as it is. Every
  system surveyed changes the average only on inbound stock.

Units sold carry `A` into their ledger rows, so:
- **cost of goods sold** = Σ(`sold` rows × `unit_cost`) − Σ(`restocked` rows ×
  `unit_cost`);
- **stock value** = Σ over items of `on_hand × A`.

Neither changes later. FIFO can be recomputed from `received` and `sold` rows if a
merchant ever needs it.

### 8.3 Late costs

- A cost row added or changed on a PO that has receipts is applied to the units already
  received. The extra per unit (§4.3) changes, and the difference times the units
  received (at each receipt's rate) is the value to add.
- **On items:** `A += value ÷ Q` when `Q > 0`. The part belonging to units already sold
  is not pushed into cost of goods sold (Odoo's rule).
- **Recording:** a `cost_adjustment` row with source `PurchaseOrder`.
- **Bookkeeping:** the PO keeps, per line, the extra value already applied, so the next
  edit applies only the difference. Receipts keep their snapshots.
- Allowed while the PO is `ordered` or `closed`.

### 8.4 The base currency (PU7)

- `Store` gains `currency: Option<Currency>`.
- **Set it** with `PATCH /admin/store` or in the setup wizard.
- **Changing it** is refused once any cost was recorded (`409 store_currency_locked`).
- **Commands that record a cost** refuse while it's unset (`409 store_currency_required`):
  setting a cost, ordering a PO, receiving.
- **The migration** sets it when every market has the same currency, and leaves it
  unset otherwise. The admin then asks for it before purchasing.
- **Commands get it as a plain value**, passed by the host, as markets' currencies are
  passed to discounts. So neither leaf depends on `hypha-stores`.

### 8.5 Cost on order lines (PU14)

- `OrderLine` gains `unit_cost: Option<UnitCost>`. Checkout copies it from the line's
  item, already loaded for reservations; untracked variants without an item have none.
- Past orders get none (migration).
- Per-order margin reads it.
- Cost of goods sold over time reads the ledger, which is what happened when the units
  left.

## 9. Incoming stock (PU8)

- **Incoming** for (variant, location) =
  - Σ `outstanding` over lines of `ordered` POs whose destination is the location, plus
  - Σ `shipped − received − rejected` over lines of `in_transit` transfers to it.
- **The earliest `expected_at`** comes with it.
- It's computed by a repository read in each leaf (`PurchaseOrderRepo::incoming(variants,
  location)`, `TransferRepo::incoming`), composed in `hypha-http` for the admin's
  inventory and variant responses (`incoming`, `next_expected_at` per level).
- It's never stored on the item: every PO edit, close or order would otherwise touch the
  items it names and race with checkouts on their version.
- Storefronts don't use it in v1. "Back in stock around…" and preorders can read the
  same function later.

## 10. Sending the PO (PU15)

- **`POST /admin/purchase-orders/{id}/send`** (`purchasing:write` and `purchasing:send`)
  takes `{ to?: [email], cc?: [email], message?: string }`.
  - The default recipient is the supplier's primary contact; with none,
    `422 supplier_email_required`.
  - It orders a draft first, then records `purchase_order.sent` with the recipients and
    the revision.
- **The `Mailer`** answers that event. It renders a new template, `supplier_purchase_order`,
  for each recipient, queued and sent by the `send_notifications` job like every other
  email. It's idempotent by (event, template, recipient).
- **The template** (Jinja, editable, previewable, test-sendable like the others) has:
  - the store's branding;
  - the PO's number, revision, date, expected date, terms;
  - the ship-to location's address, and `note_to_supplier`;
  - the lines (title, variant, SKU, supplier SKU, quantity, unit cost, line total), the
    costs and the totals in the PO's currency, formatted.
  - Its context's variables are a contract, as for the other templates.
- **Records:** `GET /admin/notifications` gains a `purchase_order` filter.
- **Printing:** the admin's PO page has a print view (`/purchase-orders/{id}/print`,
  print styles) that the browser saves as a PDF.
- **Later:** a PDF attachment (a PDF generator and attachments on `EmailSender`), and a
  link with a token to a public PO page, the first step to a supplier portal.

## 11. Scopes, cost visibility and agents (PU13)

**Scopes**
- `purchasing:read` / `purchasing:write`: suppliers, their catalogue, POs, receipts,
  setting costs. The write scope implies the read scope.
- `purchasing:send`: emailing a supplier. It needs `purchasing:write` too, and is listed
  in `hypha_agents::PROPOSE_ONLY`: an agent may draft a PO but a person sends it.
- Transfers: `inventory:read` / `inventory:write`.

**Cost visibility**
- These fields are absent unless the caller holds `purchasing:read`, staff sessions
  included:
  - inventory items' `unit_cost`;
  - ledger rows' `unit_cost` and `average_after`;
  - order lines' `unit_cost`;
  - supplier catalogue costs.
- Unlike personal data, cost hides from staff whose role lacks the scope (Shopify's
  "view product costs" permission).
- It goes through `redact.rs`'s mechanism with a new `Family` rule keyed on the scope,
  and is listed in `redacted` like personal fields. The route test
  `no_admin_route_shows_personal_data_without_its_scopes` gains a cost case.
- Storefronts never see cost.

**Other notes**
- **Supplier contacts are business data,** gated by `purchasing:read`, not by
  `customer_data:*`.
- **Agents:** every new route is an operation through the route table. Writes go through
  the agent's grant as usual. Sending is propose-only by scope.

## 12. API, events, admin

### 12.1 Routes

| Route | Scope | Notes |
| --- | --- | --- |
| `GET`, `POST /admin/suppliers` | purchasing | List filters: `archived`, `q` |
| `GET`, `PATCH`, `DELETE /admin/suppliers/{id}` | purchasing | Merge patch; `DELETE` only unused |
| `GET /admin/suppliers/{id}/variants` | purchasing:read | The catalogue, paged |
| `PUT`, `DELETE /admin/suppliers/{id}/variants/{variant_id}` | purchasing:write | |
| `GET /admin/variants/{id}/suppliers` | purchasing:read | Who sells it, at what |
| `GET`, `POST /admin/purchase-orders` | purchasing | List filters: `supplier`, `status`, `receipt_status`, `destination`, `variant`; sorted by number or `expected_at` |
| `GET`, `PATCH`, `DELETE /admin/purchase-orders/{id}` | purchasing | `PATCH` replaces `lines`/`costs` when given (merge patch); `DELETE` drafts only |
| `POST /admin/purchase-orders/{id}/order`, `…/close`, `…/cancel` | purchasing:write | |
| `POST /admin/purchase-orders/{id}/send` | purchasing:write + send | §10 |
| `POST /admin/purchase-orders/{id}/receipts` | purchasing:write | `Idempotency-Key`; `{ location?, lines, allow_over?, note? }` |
| `GET /admin/purchase-orders/{id}/receipts` | purchasing:read | |
| `GET /admin/receipts/{id}`, `POST /admin/receipts/{id}/cancel` | purchasing | |
| `GET`, `POST /admin/transfers` | inventory | List filters: `status`, `origin`, `destination`, `variant` |
| `GET`, `PATCH`, `DELETE /admin/transfers/{id}` | inventory | Edits and `DELETE` on drafts |
| `POST /admin/transfers/{id}/ship`, `…/receive`, `…/cancel` | inventory:write | `Idempotency-Key` on ship and receive |
| `GET /admin/inventory-items/{id}/movements` | inventory:read | §7.4 |
| `PUT /admin/inventory-items/{id}/cost` | purchasing:write | `{ unit_cost, note? }` |
| `POST …/levels/{location_id}/adjustments` | inventory:write | Gains `reason`, `note` |
| `PATCH /admin/store` | (as today) | Gains `currency` |

Inventory, variant and product inventory responses gain:
- `unit_cost` (redacted without `purchasing:read`);
- per level, `incoming` and `next_expected_at`.

### 12.2 Events

Each payload is a contract type, version 1:

| Type | Payload |
| --- | --- |
| `supplier.created`, `supplier.updated` (fields), `supplier.deleted` | supplier |
| `purchase_order.created`, `.updated` (fields, revision), `.ordered`, `.sent` (recipients, revision), `.closed`, `.cancelled` | purchase order, supplier |
| `purchase_order.receipt_status_changed` | from, to |
| `receipt.created`, `receipt.cancelled` | receipt, purchase order, location, lines (variant, accepted, rejected) |
| `transfer.created`, `.updated`, `.shipped`, `.received` (lines this time), `.cancelled` | transfer, origin, destination |
| `inventory_item.cost_changed` | item, variant, unit cost, why (`received`, `manual`, `late_cost`, `receipt_cancelled`) |

- Apps receive purchasing events only with `purchasing:read`, and transfer events with
  `inventory:read` (`event_scope`).
- Inventory items keep recording `inventory_item.changed`; ledger rows aren't events.

### 12.3 Admin

- **Navigation:** a "Purchasing" group in `NAV_SECTIONS`' Store section: Suppliers,
  Purchase orders, Transfers.
- **Suppliers:**
  - list;
  - page with contacts, terms, the catalogue tab (variant picker, SKU, cost, minimum
    quantity, lead time) and its POs.
- **Purchase orders:**
  - list with status and receipt badges;
  - the PO page:
    - lines with outstanding per line, costs, totals in the PO's and the store's
      currency;
    - "Mark as ordered", "Send", "Print", "Close";
    - a "Receive" dialog: location, accepted and rejected per line prefilled with
      what's outstanding, an idempotency key per dialog, a confirmation when over;
    - a receipts card with "Take back".
- **Transfers:** list and page ("Ship", "Receive" with the same dialog, "Cancel").
- **Inventory item:** a History tab (the ledger), a cost field, `incoming` beside each
  level.
- **Adjust dialog:** a reason select and a note.
- **Settings:** the store's currency (setup wizard and store details).

## 13. Later

- **Reorder points, safety stock, suggested reorders.** They read incoming (§9) and
  supplier lead times.
- **Supplier portal** and a public PO link; PDF attachments.
- **Bills, payments to suppliers, 3-way matching;** accounting sync (a QuickBooks/Xero
  spore reads the ledger, receipts and POs).
- **FIFO, lots, serials, expiry, bins;** multi-step receiving with quality control.
- **Returns to suppliers;** a transfer's return to origin; several shipments per
  transfer; holding stock at the origin before shipping (a reservation whose reference
  is the transfer).
- **Approvals for POs;** purchase units and pack sizes; supplier price breaks and
  validity dates.
- **Landed cost by weight or volume;** a cost from another supplier's bill.
- **`damaged` and `quality_control` counters** (the ledger's `quantity` column is ready).
- **Back-dated receipts and counts;** a configurable PO number prefix; merchant-defined
  adjustment reasons.
- **Storefront use of incoming** ("back in stock around…"), preorders.

## 14. Hard to reverse, and filled in while writing

**Hard to reverse (accepted):**
- **Separate POs and transfers sharing a receive step (PU3).** Tables, routes, events
  and admin flows follow from it.
- **Receipt records plus line counters (PU4).**
- **The ledger written by the item (PU5):** its columns, the closed reasons, and every
  stock write passing a cause from now on.
- **Moving average per item (PU6)**, softened by the ledger.
- **The base currency (PU7):** every recorded cost is in it.
- **Status and reason enum values; scope names; event types.** These are contracts.
- **`OrderLine::unit_cost` (PU14):** it can't be backfilled.

**Filled in while writing (confirmed 2026-11-01):**
- **`UnitCost`, a new core type: a cost per unit carries 4 decimals beyond the
  currency's minor unit.**
  - Why: a screw bought at €0.0035, or an average after mixed receipts, isn't a whole
    cent. Rounding every average to cents drifts on cheap, high-volume items.
  - Storage: `i64` in 1/10,000 of a minor unit, within ±(2⁵³ − 1).
  - The API: `{ amount, currency }` like `Money`, with `amount` a decimal string of
    **minor units**, up to 4 places (`"0.35"` is €0.0035, `"1050"` is €10.50), so it
    reads in the same unit as every other amount.
  - Line totals and stock values are rounded to `Money` half away from zero.
  - **Hard to reverse:** storage and API shape.
- **`ExchangeRate`, a new core type:** a positive decimal with 8 places, stored as
  `i64`. Conversion respects each currency's minor units.
- **One line per variant** on a PO and a transfer, so lines need no IDs. Two lines of
  one variant at different prices are written as one at an average, or as two POs.
- **`MAX_LINES` 1,000** per PO and per transfer; 20 cost rows; 10 contacts.
- **Receiving creates a missing item untracked** (§5 step 4).
- **`purchasing:send` as its own scope,** so sending can be propose-only for agents.
  `PROPOSE_ONLY` works by scope, not by operation.
- **Rejected units count as accounted for** on a line (§4.1).
- **Cost hidden from staff without `purchasing:read`** (§11), unlike personal data.
- **Transfers ship whole** in v1; receiving is in parts.
- **No ledger rows for reservations** (§7.3).

## 15. Layering

- **`hypha-purchasing` is a leaf:** core only. It names variants and locations by their
  core IDs, and takes the store's currency and the variants' titles as plain values.
- **`hypha-receiving` is a workflow:** `hypha-purchasing`, `hypha-inventory`, core.
  New edges (approved): receiving → purchasing, receiving → inventory.
- **`hypha-mail` → `hypha-purchasing`** for the template's context. A new edge
  (approved; workflow → leaf, as mail → orders).
- **`hypha-inventory`** gains transfers, the ledger and cost. Its own crate, no new
  edge.
- **`hypha-dispatch`'s** variant cleanup also removes supplier catalogue rows (dispatch
  → purchasing: a workflow → leaf edge, approved), keeping cleanups together rather than
  adding a handler to `hypha-purchasing` itself.
- **`hypha-orders`** gains `OrderLine::unit_cost`, and **`hypha-checkout`** copies it.
  No new edge: checkout already depends on inventory.
- **`hypha-stores`** gains `currency`. **`hypha-http`** composes incoming, redacts cost,
  and gains the routes.
- **No new external crates.**

## 16. Build order

Each step ends green: fmt, clippy (native and wasm32), tests, the wasm build, Postgres
contracts, e2e. Each comes with its routes, OpenAPI, SDK, e2e section and admin screens
where it has them.

1. **Core:** IDs, scopes, `UnitCost`, `ExchangeRate`; the store's currency (migration,
   route, setup wizard).
2. **The ledger:**
   - Causes on every item write; `StockMovement` recorded and written by
     every adapter path, held by the `stock_movements` contract.
   - The opening rows (migration).
   - `fulfill` and `cancel_fulfillment` pass their sources; `reason` and `note` on
     adjustments.
   - The movements route; the admin's History tab and adjust dialog.
   - **This ships on its own.**
3. **Cost:**
   - `unit_cost` on items, set by hand, recorded on rows.
   - `inventory_item.cost_changed`.
   - `OrderLine::unit_cost` at checkout.
   - Redaction by `purchasing:read`.
4. **`hypha-purchasing`, suppliers:** the aggregate, the catalogue rows, the cleanup
   handler, migrations, contracts, routes, admin.
5. **`hypha-purchasing`, POs:**
   - The aggregate, numbering, statuses, edits, costs and allocation (unit tests for
     every rounding case).
   - Migrations, contracts, routes, admin list and page.
6. **`hypha-receiving`:**
   - `receive` and `cancel_receipt` with the moving average.
   - Race tests on Postgres: two receipts of one PO, a receipt against a checkout on the
     same item, a big receipt during checkouts.
   - The receive dialog and receipts card.
7. **Incoming:** the reads, the composed responses, the admin's columns.
8. **Sending:** the template and context, the `Mailer` arm, the `send` route,
   `PROPOSE_ONLY`, the print view.
9. **Transfers:** the aggregate, ship and receive, migrations, contracts, race tests,
   admin.
10. **Late costs** (§8.3).
11. **The demo seed:** suppliers with catalogues, received and open POs, a transfer in
    transit (`pnpm seed --purchasing` for a seeded store). Then the site's roadmap moves
    purchasing to "Now".

## As built

All of §16 was built on 2026-11-01, one stacked branch per step, then reviewed and
tested end to end. Where the code differs from the sections above, or settles what they
left open:

- **Crates and tables.** `hypha-purchasing` (suppliers, catalogue rows, purchase orders,
  receipts, the allocation and landed cost in `costs.rs`, incoming in `incoming.rs`) and
  `hypha-receiving` (`receive`, `cancel_receipt`, and the purchase order edits and close
  that revalue stock). Transfers are in `hypha-inventory` (`transfer.rs`,
  `transfer_commands.rs`). Migrations 0088 (the store's currency) to 0097
  (`sent_revision`).
- **Expected dates** (`expected_at` on purchase orders and transfers) are a `Timestamp` at
  noon UTC, since core has no date type, and are always shown as their UTC day: in the
  admin, the print view and the supplier's email.
- **Late costs** (§8.3) are applied by `hypha-receiving`'s `update_purchase_order` and
  `close_purchase_order` (closing early cancels what's outstanding, so the costs spread
  over fewer units), and `cancel_receipt` keeps the rest of the order's stock right too.
  Each line keeps `extra_applied`, the extra-cost value its received stock carries in the
  store's currency. `hypha-purchasing`'s own `update_purchase_order` and
  `close_purchase_order` refuse an order goods arrived on (`purchase_order_received`), so
  only the workflow's can change one.
- **Units received past the ordered quantity** (`allow_over`) carry no extra cost: the
  extras are spread over what was ordered (`PurchaseOrder::extra_bearing`), so stock never
  carries more of a cost than was paid. Such a receipt line's landed cost is the blend of
  both.
- **A removed variant** on an ordered purchase order is still received on the order and
  the receipt (the host passes `removed_variants`), but no stock record is made for it.
- **The store's currency** is locked once a ledger row has a cost *or* a purchase order is
  ordered, closed, or a draft with a rate (`PurchaseOrderRepo::store_currency_named`), not
  only once a cost is recorded (§8.4). The check reads before the store's write, outside
  its transaction.
- **Quantities** on purchase order, receipt and transfer lines are at most
  1,000,000,000 (`422 quantity_too_large`): their columns are `integer`.
- **Sending** (§10): `purchase_order.sent` carries a `ship_to` snapshot (mail can't read
  inventory); the order keeps `sent_revision`, which the admin compares with `revision`
  for "Send updated PO"; extra costs paid in the store's currency are left off what the
  supplier sees; resending a supplier email needs `purchasing:write` and
  `purchasing:send`, like sending, so an agent can't email a supplier through
  `notifications:write`; the `supplier_purchase_order` template can't be turned off.
- **Incoming** (§9) is computed for every location of the asked variants
  (`PurchaseOrderRepo::incoming(variants)`, `TransferRepo::incoming`), merged in
  `hypha-http`'s `incoming_of`; inventory responses also name the variant's `product`.
- **Transfers** (§6): lost units are a `shrinkage` row with delta 0 whose note says how
  many; a variant whose stock record went after drafting fails at shipping
  (`variant_not_stocked`); the admin lists Transfers under Purchasing, shown with
  `purchasing:read`, though the routes need only the inventory scopes.
- **Lost races** (§5): with `MAX_ATTEMPTS` 3, a receipt or a shipment can lose every
  attempt to many concurrent checkouts on the same items; the Postgres race tests check
  that whatever wins is written whole, not that the receipt always wins.
- **Seed**: `pnpm seed` and `pnpm seed --purchasing` add three suppliers, purchase orders
  in every state (found again by their supplier reference, so an interrupted run is
  finished by the next), a transfer in transit and costs set by hand.
