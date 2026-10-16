# 0018: Fulfilment and cancellation

- **Status:** Accepted (2026-10-16). Danilo set the frame: fulfilment and order
  cancellation together; the work per location derived from reservations, not stored;
  tracking entered by hand first, with the label port reserved; a leaf crate for the
  aggregate and a workflow crate for the commands. He then accepted every
  recommendation, including section 10's (marked *Decided*).
- **Scope:** recording what was shipped (fulfilments, with tracking), taking the stock
  out of inventory as it ships, the order's `fulfillment_status`, cancelling a
  fulfilment that was recorded by mistake, cancelling an order (releasing its stock),
  their events, routes and admin screens.
- **Not in scope:** refunds and payment changes (payment spores), returns and
  restocking returns, label buying and carrier tracking updates (the provider port,
  reserved in section 8), stored fulfilment orders and warehouse (3PL) apps (section 9),
  order edits, customer-facing order pages (customer accounts, 0008 §9), UCP's order
  updates.
- **Related:** 0004 (reservations keyed by order line, I8; splitting across locations,
  I9; "fulfilment subtracts the reservation from on hand"), 0007 (split statuses, O3;
  §10's route style question for order actions), 0015 (the order's shipping snapshot and
  a quote's `reference`), 0016 (events, webhooks, Q5: items of removed variants wait for
  fulfilment), 0017 (settings).

## Decisions

| | Decision |
| --- | --- |
| FU1 | **A `Fulfillment` aggregate** (`ful_…`) in a new leaf crate **`hypha-fulfillment`**: one order, one location, the lines and quantities that left, and tracking. *Decided (crate split)* |
| FU2 | **A new workflow crate** composes orders, fulfilments and inventory: `fulfill`, `cancel_fulfillment` and `cancel_order` each write the fulfilment, the order and the inventory items in **one batch**, retried on a lost race like checkout. *Decided: `hypha-order-actions` (Q1)* |
| FU3 | **The order is the unit of consistency for quantities.** Each order line keeps `fulfilled_quantity`; two fulfilments of the same order conflict on its version, so a line can't be fulfilled twice over. `fulfillment_status` is derived from the lines and stored |
| FU4 | **No stored fulfilment orders.** What's left to ship at each location is computed from the order's reservations (0004 I9 already split them by location). Shopify-style fulfilment orders come with warehouse apps (section 9). *Decided* |
| FU5 | **Fulfilling consumes the reservation** at the fulfilling location and lowers `on_hand` there. Another location may be chosen: its `on_hand` drops and the reservation elsewhere is released. Untracked items change no stock |
| FU6 | **Tracking is entered, not fetched**: carrier, number and URL per fulfilment, by staff or an app with the scope. Label buying is a later provider port (section 8). *Decided* |
| FU7 | **Cancelling an order** releases every open reservation, stamps `cancelled_at` and a reason, and is refused while any fulfilment stands (cancel those first). Payment is untouched: refunds come with payment spores (Q3, *Decided*) |
| FU8 | **Order actions are `POST` sub-resources**: `POST /admin/orders/{id}/fulfillments`, `POST /admin/orders/{id}/cancel`, as `POST …/paid` already is. Settles 0007 §10's question. *Hard to reverse* |
| FU9 | **New scopes `fulfillments:read` and `fulfillments:write`**, apart from `orders:write`, so a shipping app can record shipments without editing or cancelling orders (Shopify does the same). Cancelling an order needs `orders:write` (Q4, *Decided*) |

*Decided while building (2026-10-16), FU10:* **every store has a location.** A
fulfilment names one, so on first start the host creates "Default" (serving no market,
like a store that hasn't set up stock) when there is none, as Shopify stores always
have one (`ensure_default_location` in `hypha-inventory`, called next to the owner's
setup). `GET /admin/locations` lists them for the fulfil dialog.

## 1. What others do

**Shopify** ([`FulfillmentOrder`](https://shopify.dev/docs/api/admin-graphql/latest/objects/FulfillmentOrder), [`Fulfillment`](https://shopify.dev/docs/api/admin-graphql/latest/objects/Fulfillment), [`orderCancel`](https://shopify.dev/docs/api/admin-graphql/latest/mutations/orderCancel))
- Each order is split into **fulfilment orders**, one per location that will ship part
  of it: the work, with its own status (open, in progress, on hold, closed, cancelled).
  Warehouse apps (fulfilment services) accept or reject the ones routed to them.
- A **fulfilment** is what actually shipped: line items and quantities from one
  fulfilment order, with tracking (company, numbers, URLs) and a status.
- Fulfilment permissions are separate scopes from order permissions.
- Cancelling an order takes a reason, and options to refund, restock and notify the
  customer.

**Medusa v2** ([fulfillment module](https://docs.medusajs.com/resources/commerce-modules/fulfillment), [order concepts](https://docs.medusajs.com/resources/commerce-modules/order))
- A fulfilment has items, a location, a provider, labels (tracking number, URL), and
  `shipped_at`, `delivered_at`, `canceled_at`.
- Order items count `fulfilled_quantity`, `shipped_quantity`, `delivered_quantity`; the
  order's fulfilment status is derived from them.
- A fulfilment provider (an interface modules implement) creates and cancels
  fulfilments with the carrier and returns its documents.
- Creating a fulfilment deducts the reservations' stock; cancelling one puts it back.

**What we take:** fulfilments as their own records with tracking (both); fulfilled
quantities on the order lines, the status derived from them (Medusa); separate
fulfilment scopes and cancellation reasons (Shopify); cancelling a fulfilment puts the
stock back (Medusa).

**What we don't take, yet:** Shopify's stored fulfilment orders (FU4) and Medusa's
shipped and delivered quantities (a fulfilment is recorded when it ships; `delivered_at`
comes with tracking updates from providers).

## 2. Model

**`Fulfillment` (`hypha-fulfillment`, a leaf on core; `ful_…`)**
- `order: OrderId`, `location: LocationId`, `lines: Vec<FulfillmentLine>` (at least
  one; `{ line: OrderLineId, quantity: Quantity }`, one entry per line).
- `tracking: Vec<Tracking>`, at most 10: `{ carrier: Option<String>, number: String,
  url: Option<String> }` (URL `http(s)://`, lengths capped like other names).
- `status`: `Shipped` | `Cancelled { at }`. `created_at` (from `Stamps`) is when it was
  recorded.
- It knows nothing about quantities ordered or stock: those are cross-aggregate rules,
  checked by the workflow (FU2).
- Repository: `FulfillmentRepo` (`get`, `list_for_order`, `update` for tracking) and
  `StageFulfillments` (insert, update) for batches.

**`Order` (`hypha-orders`) gains**
- `OrderLine::fulfilled_quantity: u32` (0 at checkout), counting fulfilments that stand.
- `fulfillment_status`: `Unfulfilled` | `PartiallyFulfilled` | `Fulfilled`, recomputed
  whenever a line's count changes, and stored for lists and filters.
- `cancelled: Option<Cancellation { at, reason }>`, `reason` one of `customer`,
  `inventory`, `fraud`, `declined`, `staff`, `other` (Shopify's list).
- Methods: `fulfil(lines) -> Result<(), OrderError>` (refuses more than is left, and
  cancelled orders), `unfulfil(lines)` (for a cancelled fulfilment), `cancel(at,
  reason)`. `mark_paid` refuses a cancelled order.

**`InventoryItem` (`hypha-inventory`) gains**
- `fulfil(reference, location, quantity)`: takes `quantity` from the reservation
  `reference` holds at `location`, and lowers `on_hand` there by the same. When the line
  is fulfilled from another location (FU5): `on_hand` drops at the chosen location and
  the same quantity of the reservation is released where it was. Partial fulfilment
  leaves the rest reserved. Untracked: nothing.
- `restock(reference, location, quantity)`: the reverse, for a cancelled fulfilment
  (stock back at its location, reserved again for the line).
- `retired: bool`: set by 0016's `variant_cleanup` when the variant is gone but units
  are still reserved. The workflow deletes a retired item once its last reservation is
  consumed or released (0016 Q5), so fulfilment never needs to ask products whether a
  variant exists.

**What's left to ship (FU4).** A function, not a record: for each line, the quantity
not yet fulfilled, by location, from the line's reservations (lines of untracked items:
unassigned, any location). The order response carries it (section 6), so the admin can
offer "Fulfil from Warehouse A: 2 × Shirt M".

## 3. Commands (`hypha-order-actions`)

```rust
pub struct Fulfil {
    pub order: OrderId,
    pub location: LocationId,
    pub lines: Vec<(OrderLineId, u32)>,
    pub tracking: Vec<RawTracking>,
    pub notify: bool,
    pub idempotency: Option<IdempotencyRequest>,
}

pub async fn fulfill<S: FulfilmentPorts>(
    ctx: &Ctx, ports: &S, ids: &impl IdGenerator, clock: &impl Clock, input: Fulfil,
) -> Result<Idempotent<Fulfillment>, CommandError>
```

`fulfill`, in a retry loop (`MAX_ATTEMPTS`), like checkout:
1. `ctx.require(Scope::FulfillmentsWrite)?`; replay if the key was used.
2. Load the order. Refuse if cancelled (`order_cancelled`), if a line isn't the order's
   (`unknown_order_line`, 422), or if a quantity is more than is left
   (`over_fulfilled`, with what's left in `details`, 409).
3. Load the location and the lines' inventory items; `fulfil` each.
4. Build the fulfilment; `order.fulfil(lines)`.
5. Stage the fulfilment (insert), the order and the changed items (update), retired
   items emptied by this (delete), the idempotency record. Commit.
6. Lost a race (`StaleVersion`): retry from 2 on fresh data, so a concurrent fulfilment
   shows as `over_fulfilled`, not as a conflict.
7. With `notify`, email the buyer a shipping confirmation through `Notifier` (logged for
   now, like invites), after the commit, in `state.background`.

`update_fulfillment_tracking` (`fulfillments:write`): replaces the tracking of a standing
fulfilment; one aggregate, `FulfillmentRepo::update`. A later `notify` sends an update.

`cancel_fulfillment` (`fulfillments:write`): marks it cancelled, `order.unfulfil`, each
item's `restock`, in one batch. Refused on a cancelled order (nothing to put back to).

`cancel_order` (`orders:write`, Q4): refused with standing fulfilments
(`order_has_fulfillments`, 409, listing them); otherwise `order.cancel`, and `release`
every line's reservation, deleting retired items emptied by it, in one batch. Cancelling
a cancelled order changes nothing (a retried request is harmless).

## 4. Events

| Type | Payload (subject and what changed) | Recorded by |
| --- | --- | --- |
| `fulfillment.created` | fulfilment, order, location, lines (line, quantity), tracking | `fulfill` |
| `fulfillment.updated` | fulfilment, order, tracking | tracking changed |
| `fulfillment.cancelled` | fulfilment, order | `cancel_fulfillment` |
| `order.fulfillment_status_changed` | order, status (from, to) | order, when its status moves |
| `order.cancelled` | order, reason | `cancel_order` |

Inventory items already record `inventory_item.changed` (and `.deleted`) on every write.
Each payload is a contract type in its crate's `events.rs`, version 1.

## 5. Changes to existing crates

- **`hypha-core`:** `FulfillmentId` (`ful_`); scopes `fulfillments:read`,
  `fulfillments:write`.
- **`hypha-orders`:** fulfilled quantities, the status, cancellation, `StageOrders`
  update (it has insert), events; the list's filter by `fulfillment_status` later.
  Fixes the comments that still say fulfilment is 0017.
- **`hypha-inventory`:** `fulfil`, `restock`, `retired`; `variant_cleanup` (in
  `hypha-dispatch`) sets `retired` instead of leaving the item as it is.
- **`hypha-fulfillment` (new leaf), `hypha-order-actions` (new workflow):** CLAUDE.md's layer
  table gains both (the workflow depends on orders, fulfillment, inventory and core).
- **`hypha-testkit`:** in-memory fulfilment repo, the `fulfillment_repo` contract, the
  batch contract's new staged kinds.
- **`hypha-store-postgres`:** a migration: `order_lines.fulfilled_quantity` (default 0),
  `orders.fulfillment_status`'s check widened (it allows only `unfulfilled` today),
  `orders.cancelled_at` and `cancel_reason`,
  `fulfillments`, `fulfillment_lines`, `fulfillment_tracking`,
  `inventory_items.retired`.
- **`hypha-host-axum`:** the routes below; error mapping; the e2e script's
  `## fulfilment` section.

## 6. API and admin

| Route | Scope | Notes |
| --- | --- | --- |
| `GET /admin/orders/{id}` | `orders:read` | Lines gain `fulfilled_quantity` and `unfulfilled` (`[{location, quantity}]`); the order gains `cancelled_at`, `cancel_reason` |
| `POST /admin/orders/{id}/fulfillments` | `fulfillments:write` | `Idempotency-Key`; body: location, lines, tracking, notify |
| `GET /admin/orders/{id}/fulfillments` | `fulfillments:read` | Newest first |
| `PATCH /admin/fulfillments/{id}` | `fulfillments:write` | Tracking (merge patch), notify |
| `POST /admin/fulfillments/{id}/cancel` | `fulfillments:write` | |
| `POST /admin/orders/{id}/cancel` | `orders:write` | Body: reason, notify |

**Admin:** the order page gets a "Fulfil" dialog (the location, prefilled with what's
left there; quantities; tracking; notify), a fulfilments card per shipment (tracking
links, edit, cancel), a "Cancel order" action with a reason, and the fulfilment status
in the orders list.

**Settings (0017):** none needed for this. Whether to notify by default is a merchant
setting (CF1), so it waits for merchant settings in the database; until then the
admin's checkbox starts checked.

## 7. Build order

Each step ends green (fmt, clippy, tests, wasm, Postgres contract, e2e).

1. Core IDs and scopes; `Order` quantities, status and cancellation; inventory's
   `fulfil`, `restock` and `retired`, each with unit tests.
2. `hypha-fulfillment`: the aggregate, repo, in-memory repo and contract, Postgres.
3. `hypha-order-actions`: `fulfill`, `cancel_fulfillment`, `cancel_order`, tracking
   updates, with race tests on Postgres (two fulfilments of one order, fulfil against
   cancel).
4. `variant_cleanup` retires items; the workflow deletes them.
5. Host routes, OpenAPI, SDK, e2e.
6. Admin screens.

## 8. Label buying, reserved

0015 left a quote's `reference` on the order's shipping lines for this. Later, the
providers that quote rates gain a second port, a signed `POST` to the app as for rates:
given the shipping line's `reference`, the location's address and the lines, the app
buys a label and returns tracking and a label file (stored through `hypha-media`). The
fulfilment then carries `label: Option<AssetId>` and `provider`. Tracking updates
(`delivered_at`, exceptions) arrive the other way, through an app calling
`PATCH /admin/fulfillments/{id}` or a dedicated route. Nothing in sections 2 to 6 needs
to change for it.

## 9. Later

- **Stored fulfilment orders** (FU4), when warehouse apps need work to accept or reject,
  or holds (fraud review, pre-orders) need a place to live.
- Returns and restocking returned units; refunds with payment spores (and the
  "refund on cancel" choice).
- `delivered_at` and tracking updates from carriers; order edits.
- A storefront or customer view of shipments; UCP order updates.

## 10. Open decisions

**Q1. The workflow crate's name.**
- **A. `hypha-fulfilment-flow`**: plain, but "flow" is new vocabulary.
- **B. `hypha-order-actions`**: says what it does (fulfil, cancel, later refunds and
  edits), so it's where the next order commands go too. *Decided*
- **C. Grow `hypha-checkout`** into all order workflows: one crate less, but a misleading
  name.

**Q2. Fulfilling more than one location at once.** *Decided:* one location per
fulfilment (a parcel leaves from one place); the admin dialog makes one per location
when the order is split. The alternative, several locations in one request, makes one
fulfilment per location in the same batch.

**Q3. Cancelling a paid order.** *Decided:* allowed; `financial_status` stays `Paid`,
and the admin says a refund must be made outside Hypha until payment spores exist. The
alternative refuses (`order_paid`) until refunds exist, which leaves merchants no way to
cancel paid orders for now.

**Q4. Who may cancel.** *Decided:* `orders:write`, as cancelling is a change to the
order, not a shipment. The alternative is a scope of its own (`orders:cancel`).

**Q5. Over-fulfilling an untracked or `continue_selling` line.** *Decided:* never;
`fulfilled_quantity` is capped at the ordered quantity whatever the stock does.

## 11. As built (2026-10-16)

Built on `feat/fulfilment` in section 7's order, one commit per step, plus FU10.

- **Orders** count `fulfilled_quantity` per line; `fulfil`, `unfulfil` and `cancel` are
  all or nothing and record `order.fulfillment_status_changed` and `order.cancelled`.
  `restore` rejects a line fulfilled past its quantity and a status the lines don't
  have. `mark_paid` refuses a cancelled order. The list's summary gains `cancelled_at`.
- **Inventory**: `fulfil` takes from the reservation at the location first, then from the
  line's reservations elsewhere, and lowers on hand (below zero if the count was off);
  `restock` reverses it; `retire` and `is_spent`. Batches stage `update_order` and a
  version-checked `delete_inventory_item`, with batch contract scenarios.
- **`hypha-fulfillment`**: the aggregate, `get_fulfillment`, `list_order_fulfillments`,
  `update_tracking` (returns whether it changed, for the email), the
  `fulfillment_repo` contract, migrations 0030 and 0031.
- **`hypha-order-actions`**: `fulfill` (idempotency key, one fulfilment ID across
  retries), `cancel_fulfillment`, `cancel_order` (`order_has_fulfillments` lists the
  standing ones), `order_view` for what's left by location, and `BuyerEmail`, which the
  host sends from a background slot (logged for now). Race tests on Postgres
  (`tests/fulfilment_race.rs`).
- **Host**: the routes of section 6 plus `GET /admin/fulfillments/{id}` and
  `GET /admin/locations`; `over_fulfilled` and `order_has_fulfillments` carry
  `details`. The e2e script has a fulfilment section.
- **Admin**: the order page's "Fulfil items" dialog (location, quantities prefilled
  with what waits there, one tracking number, email toggle, an idempotency key per
  dialog), a shipments card (edit tracking, take back), "Cancel order" with a reason,
  and the cancelled and fulfilment badges in the list.
- **Not done:** several tracking numbers in the admin's dialog (the API takes 10), a
  locations screen in the admin, notify-by-default as a merchant setting.
