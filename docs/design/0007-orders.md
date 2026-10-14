# 0007: Orders and checkout

- **Status:** Accepted (2026-10-14). Every decision was settled in conversation.
  Storage is Postgres only (in memory for tests): D1 was dropped as a target on
  2026-10-14 (0001), so nothing here has to fit D1's `batch()`.
- **Scope:** the `Order` aggregate, the checkout command that turns a cart into an order,
  and what checkout needs underneath: an atomic write across aggregates (`WriteBatch`),
  a `Clock`, idempotency keys, and a richer `PricedCart`.
- **Not in scope:** shipping (rates, methods, addresses), fulfillment, cancellation and
  refunds, payment providers (later spores), and the outbox and domain events. 0002 §7
  planned the outbox "with orders"; it moves to its own note, and O1 is designed so it
  can join the same atomic write.
- **Related:** 0001 §3.7 (`WriteBatch`), 0002 §7 (outbox), 0003 (tax lines), 0004 I7 and I8
  (atomic reservations, keyed by order line), 0005 C6 (find or create the customer).

## Decisions

| | Decision |
| --- | --- |
| O1 | **A `WriteBatch` port** commits inserts and updates of several aggregates atomically: one short Postgres transaction, one lock in memory. Domain crates stage writes through per-crate traits; storage adapters own the batch type (section 3) |
| O2 | **A leaf `hypha-orders` and a new top layer `hypha-checkout`.** Orders know nothing about carts; checkout composes cart, orders, inventory and customers. *Recommendation; it adds a layer to the table in CLAUDE.md* |
| O3 | **Two independent statuses**, like Shopify: `financial_status` (payment) and `fulfillment_status`. Payment spores move the first, fulfillment the second |
| O4 | **An order is a snapshot.** Lines copy product title, variant title, SKU, unit price, total and tax; the order copies market, currency, country, buyer and totals. Nothing is re-resolved on read. *Recommendation* |
| O5 | **`PricedCart` grows** market, currency and per-line title, variant title and SKU, so checkout builds the order from it alone and storefronts and UCP get the same fields |
| O6 | **A cart is `open` or `completed`**; completing it records its order. A completed cart can't change or be checked out again |
| O7 | **Idempotency keys** are a generic core service: a command that takes one records the key in the same batch as its effect, so a retry replays instead of repeating. First users: `checkout`, `create_cart`, `add_line` (section 5) |
| O8 | **A `Clock` port** in core, like `IdGenerator`; `Timestamp` is a core newtype (section 6) |
| O9 | **Order numbers from the first order** (`#1001`), from an `OrderNumbers` port (a Postgres sequence) taken before the batch; a failed checkout leaves a gap |
| O10 | **The buyer:** `cart.customer` if linked, otherwise `find_or_create` by `cart.email` (0005 C6), staged in the same batch. Checkout needs an email |
| O11 | **Reservations at checkout**: every line reserves stock with its order line ID as the reference (0004 I8), in the same batch as the order |

## 1. What others do

**Shopify** ([`Order`](https://shopify.dev/docs/api/admin-graphql/latest/objects/Order), [`OrderDisplayFinancialStatus`](https://shopify.dev/docs/api/admin-graphql/latest/enums/OrderDisplayFinancialStatus), [`OrderDisplayFulfillmentStatus`](https://shopify.dev/docs/api/admin-graphql/latest/enums/OrderDisplayFulfillmentStatus))
- Separate financial status (`PENDING`, `AUTHORIZED`, `PAID`, `PARTIALLY_REFUNDED`,
  `REFUNDED`, `VOIDED`, …) and fulfillment status (`UNFULFILLED`, `PARTIALLY_FULFILLED`,
  `FULFILLED`, …); cancellation and closing are timestamps beside them.
- Line items copy title, variant title, SKU and prices, so an order reads the same after
  the product changes.
- A human order number (`name`, `#1001`) beside the ID.
- Completing checkout creates the order; the checkout can't be completed twice.

**Medusa v2** ([order module](https://docs.medusajs.com/resources/commerce-modules/order), [cart completion](https://docs.medusajs.com/resources/commerce-modules/cart/cart-to-order))
- `completeCartWorkflow` validates the cart, reserves inventory, creates the order and
  marks the cart completed (`completed_at`). Steps compensate (undo) on failure instead
  of one transaction.
- Order statuses are split too (order status, payment status, fulfillment status).
- An idempotency key on cart completion guards against double submits.

**Stripe** ([idempotent requests](https://docs.stripe.com/api/idempotent_requests))
- `Idempotency-Key` header on any POST; the first result (even an error) is stored for 24
  hours and replayed. Reusing a key with different parameters is an error.

**What we take:** split statuses, snapshot lines and order numbers (Shopify); the
cart-completion steps and completed carts (Medusa); key-based replay with a parameter
check (Stripe).

**What we don't take:** Medusa's compensating steps. A crash between steps leaves
reservations without an order; one atomic batch can't. Stripe's byte-for-byte replay of
the first response (section 5 explains the difference).

## 2. Model

**`Order` aggregate (`hypha-orders`, `ord_…`)**
- `number: OrderNumber`, `placed_at: Timestamp`, `cart: CartId`.
- `market: MarketId`, `currency: Currency`, `country: CountryCode`,
  `taxes_included: bool`.
- `customer: CustomerId`, `email: Email` (the buyer at checkout; a later change to the
  customer's email doesn't rewrite the order).
- `lines: Vec<OrderLine>`, at least one.
- `subtotal`, `tax_total`, `total: Money`, all in `currency`; checked against the lines
  on construction and on restore.
- `financial_status`: `Pending` | `Paid` for now (`Authorized`, `Refunded`, … arrive
  with payment spores). `fulfillment_status`: `Unfulfilled` for now.

**`OrderLine` (owned by the order, `ordl_…`)**
- `product: ProductId`, `variant: VariantId`, `quantity: Quantity`.
- `product_title`, `variant_title: Option<String>` (option labels joined with " / ",
  `None` for a simple product), `sku: Option<String>`.
- `unit_price: Price`, `total: Money`, `tax: Option<OrderTax>`.
- `OrderTax { rate_ppm, class: Option<TaxClassId>, amount: Money, source }`: plain
  values copied from the cart's `TaxLine`, so `hypha-orders` doesn't depend on
  `hypha-tax`.

**Commands (`hypha-orders`):** `get_order`, `mark_order_paid` (manual payment, e.g. bank
transfer; also what a payment spore will call). Listing orders comes with the admin
work.

**`checkout` (`hypha-checkout`)**

```rust
pub struct Checkout {
    pub cart: CartId,
    /// What the buyer saw; a different total fails with `total_changed`.
    pub expected_total: Option<Money>,
    pub idempotency: Option<IdempotencyRequest>,
}

pub async fn checkout<S: CheckoutPorts, I: IdGenerator, C: Clock>(
    ctx: &Ctx, ports: &S, ids: &I, clock: &C, input: Checkout,
) -> Result<Order, CommandError>
```

Inside one retry loop (inventory's `retry_on_conflict`, `MAX_ATTEMPTS`):

1. Replay if the idempotency key was used (section 5).
2. Load and price the cart. Refuse if it's completed (`cart_completed`, naming the
   order), empty, has no email, has unavailable lines (`cart_unavailable`, listing
   them), or its total isn't `expected_total`.
3. Take an order number; generate the order and line IDs.
4. Find the customer, or build a new one from the email (O10).
5. Allocate stock for every line in memory (inventory's allocation, split out of
   `reserve_stock` as a function both use).
6. Stage: insert order, update cart (completed), update changed inventory items, insert
   the customer if new, record the idempotency key. Commit.
7. `StaleVersion` (cart, item) or `Duplicate` (customer email, idempotency key) means
   another request got in between: retry from 1 on fresh data.

**Scope:** a new `checkout` scope. A storefront needs it, but it shouldn't need
`inventory:write`, `customers:write` or `orders:write` (which will mean editing orders).
Checkout calls domain functions, not the other crates' commands, so their scopes aren't
checked again.

## 3. `WriteBatch` (O1)

0001 §3.7 reserved this. Constraints: static dispatch (so `Send` stays provable), core
can't name domain types, and transactions stay short. Without D1, an open transaction
across `.await`s in domain code would be possible, but it isn't wanted: Hyperdrive's
transaction pooling holds a server connection for the whole transaction, and the
in-memory store can't hold one at all. Domain code reads, decides, and hands over one
batch; only the adapter opens a transaction, for the commit.

```rust
// hypha-core/src/ports.rs
pub trait HasBatch {
    type Batch;
    fn batch(&self) -> Self::Batch;
    /// All staged writes or none. Fails like the single writes would:
    /// `StaleVersion`, `NotFound`, `Duplicate(key)`.
    async fn commit(&self, batch: Self::Batch) -> Result<(), RepoError>;
}

// hypha-cart/src/repo.rs, next to CartRepo
pub trait StageCarts {
    /// Stages the update and moves `cart` to its next version, which is only real once
    /// the batch commits; on failure the command drops `cart`.
    fn update_cart(&mut self, cart: &mut Cart);
}

// hypha-checkout
pub trait CheckoutPorts: CartPorts + HasOrders + HasBatch<Batch: CheckoutBatch> {}
pub trait CheckoutBatch: StageOrders + StageCarts + StageInventoryItems + StageCustomers
    + StageIdempotency {}
```

- **Adapters own the batch type.** `PgStore`'s is an enum of owned snapshots
  (`UpdateCart(CartSnapshot)`, `InsertOrder(OrderSnapshot)`, …); `commit` opens one
  transaction and runs the existing write functions in order. The adapter may depend
  on every domain crate, so the enum is allowed there. Memory does the same over its
  tables.
- **In memory**, every `Table` in an `InMemoryPorts` shares one lock, so a batch checks
  every version and key, then applies, without another write in between. Today each
  table has its own mutex.
- `InventoryItemRepo::update_many` stays for `reserve_stock` and `release_stock`; it can
  become a batch later.
- The rule in CLAUDE.md becomes "a command writes one aggregate, or several through one
  `WriteBatch`".

*Considered:* a named port per multi-aggregate write (`commit_checkout(order, cart,
items, customer)`). Simpler to read, but every new workflow (refunds, exchanges, spores)
adds a port method and two adapter implementations. Stage traits grow per aggregate,
not per workflow.

## 4. Cart changes (O5, O6)

- `PricedCart` gains `market: MarketId` and `currency: Currency`. `PricedLine` gains
  `details: Option<LineDetails { product_title, variant_title, sku }>`, `None` only when
  the product is gone. The host DTO and the e2e script follow.
- `Product::variant_title(variant) -> Option<String>` in `hypha-products`, so cart and
  any other reader name variants the same way.
- `Cart` gains `status: Open | Completed { order: OrderId }`. Every cart command refuses
  a completed cart (`cart_completed`, 409). `complete(order)` is only called by
  checkout.
- Migration: `carts.order_id uuid NULL`; a non-null `order_id` means completed, so no
  backfill is needed.

## 5. Idempotency keys (O7)

**Shape (`hypha-core`)**
- `IdempotencyKey`: a validated newtype (1 to 255 visible ASCII characters).
- `IdempotencyRequest { key, fingerprint: [u8; 32] }`. The adapter computes the
  fingerprint over its input (the host: SHA-256 of method, path and body; MCP and CLI
  hash their own input), because Rust's `Hash` isn't stable across compiler versions.
- `IdempotencyRepo::get(actor, key) -> Option<IdempotencyRecord>` and `StageIdempotency`
  to record one in a batch. A record is `{ actor, key, operation, fingerprint, result,
  created_at }`, where `result` is the ID the command produced or changed (`ord_…`,
  `cart_…`). Keys are scoped by actor, so two API clients can't collide.

**Flow in a command that takes one**
1. `get(actor, key)`. Found with the same operation and fingerprint: return the
   current state of `result` (`get_order`, `get_cart`), and the host adds an
   `Idempotent-Replayed: true` header. Found with a different fingerprint:
   `idempotency_key_reused` (422).
2. Otherwise run the command, staging the record with its writes. Because the key
   commits with the effect, a crash between them can't happen.
3. Two concurrent requests with the same key: one commit wins, the other gets
   `Duplicate(IDEMPOTENCY_KEY)` and replays through `report_duplicate`, the same pattern
   as other unique keys.

**Different from Stripe:** a replay returns the resource as it is now, not the first
response, and failures aren't recorded (a failed command wrote nothing, so a retry runs
it again). Storing response bodies would mean the domain storing host DTOs.

**Consequences**
- `add_line` and `create_cart` write through a batch (cart + key) when given a key.
- *Changed while building (2026-10-14):* records don't expire on read. Ignoring an
  expired record would leave its row in place, and the retry's insert would then hit the
  unique key, so expiry needs a delete inside the batch, which breaks "records are
  insert-only". Instead a scheduled cleanup job (later) deletes records older than 24
  hours (`created_at` is indexed for it); until then a key is remembered. Clients send
  fresh keys (UUIDs), so this only matters for storage size.
- The host's `KeyedJson` extractor (`extract.rs`) reads the `Idempotency-Key` header and
  fingerprints the request; replays carry `Idempotent-Replayed: true`.

## 6. `Clock` (O8)

- `Timestamp`: a core newtype over `i64` microseconds since the Unix epoch, UTC,
  matching Postgres `timestamptz` precision. No time zone or calendar logic in core;
  hosts format RFC 3339 at the edge.
- `trait Clock { fn now(&self) -> Timestamp; }`. The axum host uses `SystemTime`; Workers
  `Date.now()`; the testkit a `FixedClock` that tests can move forward.
- Commands take `clock: &impl Clock` like `ids: &impl IdGenerator`.

## 7. Order numbers (O9)

- `trait OrderNumbers { async fn next(&self) -> Result<OrderNumber, RepoError>; }`,
  called before the batch.
- Postgres: a sequence starting at 1001 (`nextval` is outside any transaction, so it
  never blocks another checkout). Memory: an atomic counter.
- A failed or retried checkout leaves a gap. Shopify's numbers have gaps too (deleted
  orders, test orders), and without gaps every checkout would queue on one row.
- Decided over leaving numbers out until an admin needs them: merchants and buyers
  expect one from the first order.

## 8. Changes to existing crates

- **`hypha-core`:** `OrderId` (`ord_`), `OrderLineId` (`ordl_`); scopes `orders:read`,
  `orders:write`, `checkout`; `HasBatch`; `Clock`, `Timestamp`; `IdempotencyKey`,
  `IdempotencyRequest`, `IdempotencyRepo`, `StageIdempotency`.
- **`hypha-cart`:** cart status, `StageCarts`, the `PricedCart` fields, keys on
  `create_cart` and `add_line`.
- **`hypha-products`:** `Product::variant_title`.
- **`hypha-inventory`:** allocation split out of `reserve_stock`; `StageInventoryItems`.
- **`hypha-customers`:** `StageCustomers` (insert).
- **`hypha-orders` (new leaf), `hypha-checkout` (new top layer).** CLAUDE.md's layer
  table gains "Workflows: `hypha-checkout` → composing and leaf domains, core".
- **`hypha-testkit`:** a shared lock for `InMemoryPorts`, `FixedClock`, the in-memory
  batch, order and idempotency repos, contracts `order_repo`, `idempotency_repo` and
  `batch` (all or nothing, versions and keys checked across aggregates).
- **`hypha-store-postgres`:** migrations for `orders`, `order_lines`, `carts.order_id`,
  `idempotency_keys` and the order number sequence; `PgBatch`.
- **`hypha-host-axum`:** `POST /carts/{id}/checkout`, `GET /orders/{id}`,
  `POST /orders/{id}/paid`, the `Idempotency-Key` extractor, e2e steps.

## 9. Build order

Each step ends green (fmt, clippy, tests, WASM, Postgres contract, e2e).

1. `Clock` and `Timestamp`.
2. `WriteBatch`: core trait, shared in-memory lock, batch contract, `PgBatch`. Staging
   for carts and inventory items first, tested by the contract alone.
3. `PricedCart` fields and `Product::variant_title`.
4. Idempotency keys: core types, repos, contract, then `create_cart` and `add_line`.
5. `hypha-orders`: aggregate, repo, contract, `get_order`, `mark_order_paid`, Postgres.
6. Cart status, allocation split out of `reserve_stock`, then `hypha-checkout`.
7. Host routes and e2e.

## 10. Later

- Shipping and billing addresses on cart and order (nullable columns, so adding them
  doesn't break anything), shipping rates and methods.
- Fulfillment (deducting `on_hand`, `fulfillment_status` moving), cancellation
  (releasing reservations), refunds.
- Payment provider spores moving `financial_status` (`Authorized`, `Refunded`, …).
- The outbox and domain events (0002 §7), staged in the same batch.
- Listing and searching orders; order edits.
- Surfacing the hot-item conflict at checkout: after `MAX_ATTEMPTS` retries checkout
  returns 409, which a storefront should retry.
