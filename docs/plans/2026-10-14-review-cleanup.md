# Plan: review cleanup before orders (2026-10-14)

From a four-part review on 2026-10-13 (architecture, domain logic, storage, HTTP and
docs), after tax, inventory and customers were merged (`main` at `7c5e411`). The main
claims were checked against the code; one reviewer's claim that CLAUDE.md lacks the new
crates was wrong and is left out.

The architecture holds up: crate dependencies match the layers, ports have no `Send`
bound and no `&dyn`, all 42 commands check a scope first, every `ErrorCode` match is
exhaustive, the WASM build passes, and all 10 repository contracts run in memory and on
Postgres. What follows is edge cases and debt.

Work in three phases, each on its own branch, each ending green (fmt, clippy, tests,
WASM, Postgres contract, e2e on both storages).

## Phase 1: bugs (branch `fix/review-bugs`)

Each fix starts with a failing test.

- [ ] **B1. `add_line` saves a line whose total overflows.**
  `crates/hypha-cart/src/commands.rs:194-208`. It checks the unit price (`price_line`)
  and stock, but `unit × quantity` only overflows inside `price_cart`, which marks the
  line unavailable instead of failing; then the cart is saved.
  Repro: price list +100 000 000 bp, add 1 000 000 000 mugs → `Ok`, line stored.
  Fix: after `price_line`, compute `unit.money().checked_mul(line.quantity())?`, or fail
  if the new line comes back `Unavailable` from `price_cart`.
- [ ] **B2. One overflowing cart total makes the cart unreadable.**
  `crates/hypha-cart/src/pricing.rs:148-154`. `Money::sum` overflowing fails `get_cart`
  and every later `add_line`, and there is no command to remove a line.
  Fix (decide): cap line quantity (e.g. 1 000 000) so a total can't reach 2⁵³ − 1, or
  mark lines unavailable instead of failing the cart. Capping is simpler.
- [ ] **B3. Postgres reads can mix two versions of an aggregate → 500.**
  Multi-query loads run outside a transaction (READ COMMITTED): `customers.rs:141-190`,
  `products.rs:189-263`, `carts.rs:19-60`, `markets.rs:125-145`, `inventory.rs:345-391`,
  `tax.rs:211-237`. A write between the root query and the children query can produce
  e.g. a default address that is no longer in the address book; `restore` fails and
  `corrupt()` turns it into a 500.
  Fix: run each multi-query load in `BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY`
  (a shared helper). D1 equivalent later: a read-only `batch()`.
- [ ] **B4. `set_cart_customer` leaks customer emails.**
  `crates/hypha-cart/src/commands.rs:230-250` requires only `carts:write`, then copies
  the customer's email onto the cart (visible in `GET /carts/{id}`) and reveals whether
  a `cus_…` exists. Fix: also `ctx.require(Scope::CustomersRead)?` when linking.
- [ ] **B5. `reserve_stock` accepts the same reference twice for one variant.**
  `crates/hypha-inventory/src/commands.rs` (input validation). Two lines
  `("ol-1", v, 2)` reserve 2, not 4, and return `Ok` (the second hits the idempotency
  path). Fix: reject duplicate `(reference, variant)` pairs with a typed error; same in
  `release_stock`.
- [ ] **B6. Wildcard arm in `StockContext::availability`.**
  `crates/hypha-inventory/src/stock.rs:44` (`_ => Available`): a future error would
  count as sellable. Fix: make `can_supply` return `Result<(), i64>` (it only ever fails
  with a shortage) and match it exhaustively.
- [ ] **B7. Map Postgres serialization failures.** `crates/hypha-store-postgres/src/error.rs`:
  SQLSTATE 40001 → `StaleVersion`, next to the deadlock mapping (needed once B3 adds
  REPEATABLE READ).
- [ ] **B8. Migration bootstrap isn't safe when two hosts start at once.**
  `crates/hypha-store-postgres/src/migrate.rs:147-173`: `CREATE TABLE IF NOT EXISTS
  schema_migrations` and the upgrade block run before any lock. Fix: one transaction
  that takes `pg_advisory_xact_lock(<const>)` first.
- [ ] **B9. Small correctness fixes.**
  - `Email::MAX_LEN` counts characters; SMTP's 254 is bytes (`hypha-core/src/email.rs:36`).
  - `allocate` assumes distinct location IDs (`hypha-inventory/src/item.rs:149`): dedupe.
  - `update_many` lock order relies on the caller sorting: sort by ID inside the
    adapter too (`hypha-store-postgres/src/inventory.rs`).
  - Typo "combintion" (`hypha-products/src/error.rs:27`).

## Phase 2: decisions, then refactors (branch per decision)

Decide first; each is easier now than after orders build on it.

- [ ] **D1. Split `CartPorts` into per-domain accessor traits.**
  Today every host route reaches its repos through `hypha_cart::CartPorts`
  (`hypha-host-axum/src/state.rs:40`), and the 10-port plumbing is written out 4 times
  (trait + `&T` impl, `MemoryStorage`, `PgStore`, the cart test world). Orders would
  double it.
  Proposal: `HasProducts { type Products: ProductRepo; fn products(&self) -> … }` per
  domain; `trait CartPorts: HasCarts + HasProducts + … {}` with a blanket impl; commands
  bound only what they use; hosts bound the union. Testkit ships one `InMemoryStore`
  used by the host and the tests. Static dispatch, no `Send` change.
  *Recommendation: yes, before orders.*
- [ ] **D2. One status for `out_of_stock`.** 422 from `add_line`
  (`host-axum/src/error.rs:140`), 409 from inventory (`:186-187`). *Recommendation: 409*
  (depends on current stock, not on bad input).
- [ ] **D3. One PATCH convention.** Customers clear with `""`
  (`routes/customers.rs:26`), cart routes with `null`, `PATCH /markets/{id}` and
  `PATCH /tax-regions/{id}` require every field. *Recommendation: JSON Merge Patch
  everywhere* (absent = keep, `null` = clear; a double `Option` in the DTOs) and
  `#[serde(deny_unknown_fields)]` on request DTOs, so a typo isn't a silent 200.
- [ ] **D4. Restore-only errors.** `DuplicateClassRate` maps to 409
  (`host-axum/src/error.rs:163`), inventory's and customers' to 500; all are unreachable
  because adapters wrap `restore` failures as `Backend`. Option: a separate error type
  for `restore`, so `CommandError` and the host don't carry dead arms.
- [ ] **D5. Units in field names.** `adjustment` is basis points, `rate` and
  `default_rate` are parts per million. Rename to `adjustment_bps` / `rate_ppm`? (Breaking
  for the API, which has no users yet.)
- [ ] **D6. Country input.** Carts and addresses take `CountryCode` in the DTO (bad input
  → `invalid_body`), markets and tax regions take `String` (→ `invalid_country_code`).
  *Recommendation: take `String`, validate in the command.*
- [ ] **D7. Clearing a cart's email while a customer is linked** leaves `customer` set and
  `email` empty (`hypha-cart/src/cart.rs:69`). Unlink, refuse, or allow?

## Phase 3: cleanup (branch `chore/cleanup`)

- [ ] **Shared test fakes.** 6 hand-written racing repos (`hypha-products/tests/commands.rs:179`,
  `hypha-markets/tests/create.rs:579`, `hypha-tax/tests/commands.rs:306`,
  `hypha-inventory/tests/commands.rs:241`, `hypha-inventory/tests/reserve.rs:55`,
  `hypha-customers/tests/commands.rs:154`): replace with a "hide the next lookup" switch
  in testkit.
- [ ] **Shared duplicate-key helper** for the "pre-check, map `Duplicate`, look up again"
  block (products, markets ×2, tax, inventory, customers).
- [ ] **`Quantity` to core**; inventory uses it instead of a bare `u32` with its own zero
  check.
- [ ] **One text rule** (trim, empty → none, ≤ 255): `hypha-customers/src/customer.rs:176`
  copies `hypha-core/src/address.rs:76`. Product title, market name and tax class name
  have no checks; location name isn't trimmed or capped.
- [ ] **Dead code:** `market_for`, `check_no_overlap`, `catalog_for`,
  `check_one_catalog_per_context`, `CatalogError` (markets). Keep `Market::add_country`,
  `remove_country`, `Cart::set_quantity`, `remove_line` but give them commands and routes
  (cart quantity/remove is on the roadmap anyway).
- [ ] **Cart handlers** (`routes/carts.rs` create, set_email, set_customer) run a command
  then `get_cart`: make those commands return `PricedCart`, like `add_line`.
- [ ] **One host extractor** for `(ctx, ports)` instead of 31 copies of
  `Ctx::system(); // TODO: real auth` + `ports().await?`, so auth lands in one place.
- [ ] **`ProductStatus` in DTOs** (`routes/products.rs:25,33`): map through an API enum
  (CLAUDE.md forbids serializing domain types); then products no longer needs `serde`.
- [ ] **Products crate drift:** `CommandError::NotFound` → `ProductNotFound`;
  `HandleTaken` should carry the existing product ID; `uuid` is only a test dependency;
  inline `snapshot.rs` like the other crates. Make older error enums `Clone`.
- [ ] **Product saves write row by row** (`hypha-store-postgres/src/products.rs:127-176`):
  use `unnest … WITH ORDINALITY` like the other adapters.
- [ ] **Fewer queries per request** (`get_cart` ≈ 21, `add_line` ≈ 31 round trips):
  return early on empty ID lists (`load_items`, `load_locations`); `add_line` loads the
  product twice.
- [ ] **CHECK constraints** (new migration): currency on `variants`, `markets`,
  `price_lists`, `price_list_prices`; money amounts `≤ 9007199254740991`;
  `customers.email = lower(email)`.
- [ ] **Migration checksums**: store a hash of each migration's SQL and fail on mismatch
  or on unknown applied versions (`migrate.rs:186-197`).
- [ ] **In-memory vs Postgres differences** for batch methods (missing parent, duplicate
  fixed prices, duplicate items in `update_many`, option IDs reused across products):
  define them in the contract or document them as caller preconditions.
- [ ] **Contract gaps:** unique keys on update (`HANDLE`, `TAX_REGION_COUNTRY`,
  `INVENTORY_ITEM_VARIANT`); clearing cart email/customer; product updates that remove
  or reorder options/variants.
- [ ] **Host:** route-level tests with `tower::ServiceExt::oneshot` against
  `memory_router()`; feature-gate `hypha_testkit::contract` (it ships in the binary);
  `PORT`/`BIND` env and graceful shutdown; a health route.
- [ ] **e2e gaps:** `GET /products/{id}` (200 and `product_not_found`),
  `GET /inventory-items/{id}` 200, and ~20 unexercised error codes (`invalid_handle`,
  `no_countries`, `duplicate_country`, `invalid_basis_points`, `duplicate_fixed_price`,
  `missing_content_type`, 422 `price_list_not_found` / `market_not_found` from bodies,
  `variant_not_found`, `quantity_overflow`, `field_too_long`, …).
- [ ] **Unknown IDs in bodies** are accepted on 5 routes (leaf crates can't check other
  domains: fixed prices, catalog products, tax assignments, location markets, inventory
  item variant). Document the exception in CLAUDE.md and error.rs, or validate in a
  composing layer.

### Docs

- [ ] CLAUDE.md: the `update_many` exception to "a command writes at most one aggregate";
  tracing is no longer deferred; a rule for quoting raw input with `{:?}` in error
  messages (`email.rs:17`, `money/mod.rs:41`, `products/src/error.rs:9,33`) or switch
  them to `'{0}'`.
- [ ] 0001: sqlx → tokio-postgres (line 4, §3.8), "every command writes one aggregate"
  (§3.7), "all five contracts" (now 10), "`load_products` runs one query per line",
  multi-tenancy note "before customers", "retrying on StaleVersion → 409" (inventory
  retries).
- [ ] 0002 §7: OpenAPI-derivable DTOs and API keys slipped; move the dates or do them.
- [ ] 0003: `duplicate_class_rate` mapping; customer tax exemptions are deferred in 0005.
- [ ] 0004: commands are `update_inventory_item`, not `set_tracked` /
  `set_continue_selling`; there is no `variant_inventory_items` table;
  "`continue_selling` items are always Available" (not with zero serving locations).
- [ ] `hypha-cart/src/commands.rs:287` query count; `scripts/e2e.sh:2` says in-memory only.

## Before orders (goes into design note 0007, not this cleanup)

- **A `Clock` port**: orders need `placed_at`; core and domain crates can't read time.
- **Cart status** (open / completed, link to its order), so a cart can't be checked out
  twice; checkout needs an idempotency key.
- **What an order snapshots**: `PricedCart` has no market or currency, no product or
  variant title or SKU, and no line ID to use as the reservation reference (0004 I8).
- **The checkout write**: order + cart + stock reservations + maybe a new customer.
  Reserve-then-insert with release on failure (the earlier recommendation), or the
  generic `WriteBatch` (0001 §3.7). Each in-memory `Table` has its own mutex, so the
  memory store can't commit across repos atomically.
- **Which buyer wins at checkout**: prefer `cart.customer`; otherwise
  `find_or_create_customer(cart.email)`. The cart's copied email can be stale after a
  customer changes theirs.
- **Shipping and billing address on the cart**; fulfillment (deduct `on_hand`) and
  cancellation (release); surfacing the hot-item 409 at checkout; the outbox and domain
  events planned "with orders" in 0002 §7.
