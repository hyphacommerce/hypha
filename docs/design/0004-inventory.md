# 0004: Inventory

- **Status:** Accepted (2026-10-13). I2, I3, I6, I7 and I9 were decided in
  conversation; the rest were proposed and not objected to.
- **Scope:** stock per location, availability on priced carts, and reservations that
  orders will make. Orders, fulfillment, returns and transfers between locations are
  out of scope; this note only makes sure they can call into inventory.
- **Related:** 0001 (optimistic concurrency, D1 has no interactive transactions,
  `WriteBatch` in 3.7), 0002 (closed aggregates, extensions link by ID), 0003 (the same
  "leaf crate links by ID" shape as tax classes).

## Decisions

| | Decision |
| --- | --- |
| I1 | Stock belongs to an **inventory item** owned by `hypha-inventory`, linked to a variant by ID (one item per variant for now) |
| I2 | **Several locations from the start.** Each location lists the markets it sells to, and has a priority |
| I3 | Tracking is off with `tracked: false` (Shopify). A variant with no item yet is reported and treated as `tracked: false`, so there is one concept |
| I4 | Quantities per location: `on_hand`, and `reserved` (the sum of the reservations). `available = on_hand − reserved` |
| I5 | Selling past zero is a per-item flag, `continue_selling` (Shopify's `inventoryPolicy: CONTINUE`, Medusa's `allow_backorder`) |
| I6 | Carts don't reserve. `add_line` **refuses** a quantity the market's locations can't cover; a priced cart still **marks** lines that ran out after being added |
| I7 | **Overselling is prevented with optimistic concurrency, not locks**: the item is the versioned aggregate, all of an order's reservations are written in one atomic batch, and a conflict retries the command a few times (section 3) |
| I8 | Reservations are children of the item, keyed by the caller's reference (later the order line), so reserving twice is a no-op |
| I9 | A line takes stock from the first location (by priority) that covers all of it; only if none does is it split across locations in priority order |
| I10 | Setting `on_hand` from a count can carry the quantity the caller last saw (Shopify's `changeFromQuantity`), so a sale in between isn't overwritten |

## 1. What others do

**Shopify** ([quantities and states](https://shopify.dev/docs/apps/build/orders-fulfillment/inventory-management-apps/manage-quantities-states), [`InventoryItem`](https://shopify.dev/docs/api/admin-graphql/latest/objects/InventoryItem), [`ProductVariantInventoryPolicy`](https://shopify.dev/docs/api/admin-graphql/2025-10/enums/ProductVariantInventoryPolicy))
- An `InventoryItem` sits behind each variant and has an `InventoryLevel` per location.
  `tracked` on the item turns counting off.
- Each level has named quantities. `on_hand` is the sum of `available`, `committed`
  (taken by orders), `reserved`, `damaged`, `safety_stock` and `quality_control`;
  `incoming` is separate.
- `committed` is managed by Shopify through orders; apps can't adjust it.
- Writes take a `reason` (`received`, `correction`, `shrinkage`, …) and a reference to
  what caused them, and accept an idempotency key.
- **Concurrency is compare-and-swap on the quantity itself:** `changeFromQuantity` is the
  value the caller expects, and a mismatch fails with `CHANGE_FROM_QUANTITY_STALE`.
  Passing `null` opts out; Shopify recommends that only when the caller is the source of
  truth.
- Selling past zero is the variant's `inventoryPolicy`: `DENY` or `CONTINUE`.
- Which locations count for a buyer depends on the location's "fulfill online orders"
  setting and on the regions it ships to, so stock shown in a market only comes from
  locations that serve it ([Markets inventory](https://help.shopify.com/cs/manual/markets/inventory_and_fulfillment)).

**Medusa v2** ([inventory concepts](https://docs.medusajs.com/resources/commerce-modules/inventory/concepts), [inventory in flows](https://docs.medusajs.com/resources/commerce-modules/inventory/inventory-in-flows), [locks](https://docs.medusajs.com/learn/fundamentals/workflows/locks))
- An inventory module owns `InventoryItem`, `InventoryLevel` (`stocked_quantity`,
  `reserved_quantity`, `incoming_quantity` per `location_id`) and `ReservationItem`
  (item, location, quantity, optional `line_item_id`). Stock locations are a separate
  module, and sales channels link to them.
- `manage_inventory` and `allow_backorder` live on the variant; the variant links to
  items with a `required_quantity` (so one variant can be a kit of several items).
- **Add to cart** checks stock and refuses the line if it's short, but reserves nothing.
- **Completing a cart** (`completeCartWorkflow`, read from source):
  - takes a lock on the cart ID (30 s timeout, 2 min TTL);
  - creates the order;
  - then `reserveInventoryStep` takes a second lock on the inventory item IDs and calls
    `createReservationItems`;
  - payment is authorized last; if a step fails, earlier steps are compensated (the
    reservations are deleted).
- `createReservationItems` checks `available_quantity` and then writes
  `reserved_quantity = old + n` from application code (read-modify-write), unless
  `allow_backorder` is set. **The lock is what keeps two checkouts apart.** It comes from
  a locking module whose default provider is an in-memory map, which only works on one
  server; several servers need the Redis provider.
- Fulfillment subtracts the reservation from `stocked_quantity` and deletes it; returns
  add stock back.

**What we take from them**
- An item behind each variant, a level per location, and a tracked flag (both).
- Reserve when the order is placed, not when adding to a cart (both).
- Reservations as their own records with a reference to the line (Medusa), which makes
  them idempotent and lets fulfillment find them.
- Compare-and-swap for counts that come from outside (Shopify).

**What we don't take**
- **Medusa's locks.** Hypha runs on Cloudflare Workers, which keep no shared memory
  between requests and have no Redis by default. A lock that needs extra infrastructure
  to be correct is the wrong default (section 3).
- **Shopify's full set of states** (`damaged`, `safety_stock`, `quality_control`,
  `incoming`). They are easy to add as extra counters on a level later; `on_hand` and
  `reserved` are enough for orders.
- **Kits** (Medusa's `required_quantity`). The link has room for a quantity later.

## 2. Model (`hypha-inventory`, a leaf crate depending only on core)

**`Location` aggregate (`loc_…`)**
- `name`, `markets: Vec<MarketId>` (the markets it sells to), `priority: u32` (lower
  goes first).
- `MarketId` is a core type, so this adds no edge to `hypha-markets` (like tax regions
  hold a `CountryCode`).
- A location that serves no market still holds stock; it just doesn't count for any
  cart.

**`InventoryItem` aggregate (`invi_…`)**
- `tracked: bool`, `continue_selling: bool`.
- `levels`: one per location that stocks it, with `on_hand: i64`. `on_hand` can go
  negative only through `continue_selling` reservations being fulfilled.
- `reservations`: `{ reference, location, quantity }`, unique per (reference, location).
  `reserved` at a location is the sum of its reservations.
- It owns its levels and reservations the way a product owns its variants: one row
  group, one version, one write.
- Commands: `create_inventory_item`, `update_inventory_item` (`tracked`,
  `continue_selling`),
  `set_on_hand` (with an optional `expected` quantity, I10), `adjust_on_hand(delta)`.

**Link `variant → item`**
- The item stores its `variant`, in the inventory crate's own table (no foreign key to
  products, like `product → tax class`). A unique key allows one item per variant.
  Kits (one variant made of several items) would move this to a separate link table.
- A variant with no item is treated as `tracked: false`, and reads report it that way
  (I3). The state can't be avoided: `hypha-products` can't create items (that would be
  a leaf → leaf edge), so a new variant has no item until stock is set up. Treating it
  as "0 in stock" instead would make every existing product unsellable once inventory
  ships.

**Availability (pure function, like `tax_line`)**
- Input: the items for the cart's variants, the locations that serve the market, and
  the requested quantities.
- Output per line: `Available`, `OutOfStock { available }`, or `Untracked`.
- `available` is the **sum** over the market's locations, which is what splitting (I9)
  can actually deliver. Without splitting it would be the largest single location's
  stock, and a buyer could see 3 in stock and still be refused 3.
- `continue_selling` items are `Available` as long as at least one location serves the
  market; with none, there is nowhere to sell from, so they are out of stock too.

**Allocation (I9)**
- For each line: the highest-priority serving location that covers the whole quantity.
- If none does: take from locations in priority order until the line is covered, one
  reservation per location.
- A merchant with one location never splits. Shopify splits too, routing one order to
  several locations as separate fulfillment orders.

## 3. Preventing overselling (I7)

**The problem.** Two checkouts both read "1 left" and both reserve it. Whatever we
choose has to work on Postgres, through Hyperdrive on Workers, and on a future D1
adapter, which has **no interactive transactions** (0001, section 2).

**Options**

| | How | Works on D1 / Workers? | Verdict |
| --- | --- | --- | --- |
| A | **Locks** around check-then-write (Medusa) | Needs a lock service shared by every instance (Redis, Durable Objects). The in-memory default silently does nothing across instances | No: new infrastructure, and correctness depends on configuring it |
| B | **`SELECT … FOR UPDATE`** in an open transaction | No: D1 can't hold a transaction across awaits, and 0001 forbids it at the port level | No |
| C | **Guarded update in SQL**: `UPDATE levels SET reserved = reserved + $n WHERE … AND on_hand - reserved >= $n`, one row back means success | Yes, as a single statement or `batch()` | Possible, but the stock rule (tracked, `continue_selling`, split across locations) moves into every adapter's SQL |
| D | **Optimistic concurrency**: read the items, decide in domain code, write them with `WHERE version = $expected`; a stale version means someone else got there first, so re-read and retry | Yes: it's what every repo already does (0001, 3.2) | **Recommended** |

**Recommendation: D, with three parts.**
1. **The item is the unit of consistency.** Its levels and reservations change only
   through the item, under its version. Two checkouts for the same item can't both
   succeed; checkouts for different items never conflict.
2. **One order, one atomic write.** An order with three lines reserves three items.
   `InventoryItemRepo::update_many` writes them all or none: every item carries its
   expected version, and the write is one Postgres transaction (or one D1 `batch()`).
   If any item changed, nothing is reserved. This is the first command allowed to write
   more than one aggregate, so it's the first real use of 0001 (3.7).
   - It is scoped to inventory items on purpose. The generic `WriteBatch` from 0001,
     which mixes aggregate types (an order *and* its items), only has a user once orders
     exist, and its shape depends on theirs. It comes with orders.
3. **Retry a conflict, briefly.** `reserve_stock` re-reads and retries up to 3 times on
   `StaleVersion`, then returns `409 concurrent_modification`. A retry runs the stock
   rule again on fresh data, so a real shortage comes back as `out_of_stock`, not as a
   conflict.

**Why not C, even though it never retries?** C is faster when many buyers hit one item at
once (a drop or flash sale), because the database queues the updates instead of the
command retrying. But each adapter would hold its own copy of the rules for tracking,
`continue_selling` and location splitting, and the contract tests would be the only
thing keeping them in sync. D keeps every rule in Rust, and the adapters only need the
version check they already have. **If a hot item ever shows up in practice, C can be
added for that path later** without changing the domain model.

**Measured on Postgres (2026-10-13,
`crates/hypha-store-postgres/tests/inventory_race.rs`).** 8 buyers on separate
connections each reserve 1 of 3 units at the same moment. In every run exactly 3 win,
and nothing is ever oversold. But **the 5 losers all get `409 concurrent_modification`,
not `out_of_stock`**: they retry in lockstep, each round only one write can win, so 3
attempts run out before they see that the stock is gone. Contention between a few
buyers resolves within the retries. A hot item (a drop, a flash sale) doesn't; its
buyers see a retryable 409 instead of a precise answer. Options when that matters,
none built yet:
- jittered backoff between attempts (needs a sleep port: hosts differ, Workers included);
- more attempts (cheap, but each one is a full round of queries);
- option C above for hot items only.

**What the cart checks are for (I6).** `add_line` refuses a quantity that isn't there,
so buyers find out at once. Stock can still run out after a line was added (someone
else bought the last one, or the cart sat for days), so the priced cart also marks such
lines `out_of_stock` and leaves them out of the totals, like lines without a price. The
line stays in the cart, which an abandoned-cart or back-in-stock flow can use later.
Neither check prevents overselling; only reservation does, and only reservation is under
the version check.

**Idempotency (I8).** A retry after a timeout must not reserve twice. A reservation is
keyed by its reference (the order line) and location, so a repeated `reserve_stock` for
the same reference finds its reservation already there and changes nothing.

## 4. Changes to existing crates

- **`hypha-core`:** `LocationId` (`loc_`), `InventoryItemId` (`invi_`); scopes
  `inventory:read`, `inventory:write`.
- **`hypha-cart`:** `CartPorts` gains inventory ports; `add_line` can fail with
  `out_of_stock`; `PricedCart` lines gain `out_of_stock`. That is a new edge `hypha-cart → hypha-inventory`, which the layer
  table allows (composing domain → leaf).
- **`hypha-store-postgres`:** migration 0007 with `locations`, `location_markets`,
  `inventory_items`, `inventory_levels`, `inventory_reservations`. (No
  `variant_inventory_items`: the item stores its variant, unique per variant.)
- **`hypha-host-axum`:** routes for locations, items, levels and the variant link; the
  e2e script gets an `## inventory` section.

## 5. Build order

1. Core: IDs and scopes.
2. `hypha-inventory`: locations, items, levels, availability, commands; in-memory repos
   and contract.
3. `reserve_stock` / `release_stock` on `update_many`, with the retry and idempotency
   tests, including a test that fakes a concurrent write between read and write.
4. Postgres: migration 0007 and repos.
5. Cart: `add_line` refuses missing stock; `out_of_stock` on priced lines.
6. Host routes and e2e.

## 6. Open questions

- **The reservation reference type:** orders don't exist yet. Proposed: a validated
  newtype over a string, replaced by an order line ID once orders exist.

## 7. Later

- Fulfillment (reservation → deduct `on_hand`), returns, cancellations.
- Reservations that expire (holding stock during checkout, Medusa's ticket example).
- More quantity states (`incoming`, `damaged`, `safety_stock`), adjustment reasons, and
  a history of changes.
- Kits: a quantity on the variant → item link.
- Option C for hot items, if needed.
