# 0002: Extensibility

- **Status:** Accepted as direction (2026-10-13). Nothing here is built yet, except what
  section 7 lists. Each part gets its own implementation step (and note, if needed) when
  it's due.
- **Decided:** TypeScript developers building client stores are the primary extenders.
  Rust spores come second.
- **Related:** design note 0001 (storage), which already provides per-module migrations.

## 1. Who extends Hypha, and how

| Tier | Who | How | Runs |
| --- | --- | --- | --- |
| **External apps** (primary) | TypeScript developers building client stores and integrations | HTTP API (later MCP), a typed TS client generated from OpenAPI, webhooks, API keys with scopes | Anywhere, in any language, outside Hypha |
| **Spores** (secondary) | Rust developers extending Hypha itself (payment providers, tax engines, reviews) | Crates compiled into a host, registered through a `Spore` trait | Inside Hypha, in-process, on every host (incl. Workers) |
| **Configuration** | Merchants and admin agents | Custom field definitions and policies, as data through the API | Inside Hypha, no code |

Most needs should be met by the first and third tiers. A spore is for things that must
run inside a transaction or a request (payment providers, price or tax calculation),
or that need their own tables with Hypha's guarantees. Shopify works the same way:
apps live outside.

## 2. Principles

1. **Core aggregates are closed.** No app or spore adds columns to core tables or changes
   core types. This is Medusa v1's lesson (extending core entities broke upgrades), and
   Medusa v2's rule ("you can't access another module's data models to add a relation to
   it or extend it").
2. **Extensions link to core by ID**, without foreign keys, like Medusa v2's module
   links. Hypha already works this way: `catalog_products` and `cart_lines` reference
   products by ID only.
3. **Everything an extender sees is typed and stable:** prefixed IDs, error `code`s,
   documented JSON shapes, versioned events. Error messages are for humans and may
   change; codes may not.
4. **Behavior is extended through events and ports, not by patching commands.**
5. **Validation can only get stricter** (section 4). Core invariants are what pricing,
   storage and every extension rely on.

## 3. Custom fields (metafields)

Typed, with definitions, like Shopify's metafield definitions, and not an untyped
`metadata` JSON blob. Admin UIs, admin agents and generated clients need to know what
exists and what's valid.

- **Definition:**
  - owner type: product, variant, market, cart; later customer, order, ...
  - namespaced key: `custom.material`, `app_acme.erp_id`, `spore_stripe.customer_id`
  - type: text, integer, decimal string, boolean, money, date, reference (prefixed ID),
    or a list of one of these
  - rules: required, min/max, max length, allowed values
  - visibility: storefront-readable or admin-only
  - who may write it: admin, a given app, a given spore

  Definitions are data, created through the API, or registered in code by a spore.
- **Values** are stored outside the aggregate, in their own table keyed by
  `(owner_type, owner_id, namespace, key)`:
  - the aggregates and their invariants stay untouched
  - editing a custom field never conflicts with an edit to the product's title
  - reads are batched for many owners, like fixed prices (0001)
- **Writes** go through a `set_custom_fields(owner, values)` command, which checks every
  value against its definition. The error codes are `custom_field_unknown` and
  `custom_field_invalid`, with the field path.
- **Reads:** API responses include `custom_fields: { "custom.material": "wool", ... }`,
  filtered by visibility.
- **Later:** filtering and sorting storefront queries by custom fields needs indexes
  (expression or GIN indexes on the values table).
- **Lives in** a leaf domain crate (`hypha-custom-fields`) plus a store-postgres
  migration module.

## 4. Policies: stricter validation of core fields

Two different requests:

- **Tightening** is supported, through policies. Examples: SKU required, title at most
  70 characters, price above zero, at most 10 of an item per cart. The core rules still
  hold, and a policy adds a rule on top.
  - Configurable policies are data from a fixed catalog of rule kinds (required, length,
    range, pattern) applied to core fields, so an admin agent can switch them on.
  - Code policies come from spores: `trait Policy<Target> { fn check(&self, target: &Target) -> Result<(), PolicyViolation> }`.
  - A violation returns `policy_violation` with the rule ID and the field path.
- **Loosening** a core rule (negative prices, uppercase handles) is not possible. If a
  core rule is wrong, it's a core change with a version note.

## 5. Events, outbox and webhooks

Events are how external apps and spores react ("order paid → send to ERP") without
touching core code.

- **Domain events are recorded by the aggregate** when something happens, for example
  `ProductArchived`, `OrderPaid`. The repository writes them to an `outbox` table **in
  the same transaction** as the aggregate. On D1, that's the same `batch()`. So an event
  exists if and only if the change committed, and can't be lost or invented.
- **A dispatcher** reads the outbox and delivers each event to:
  - in-process spore handlers
  - **webhooks** for external apps: signed (HMAC), retried with backoff, at least once,
    each delivery carrying the event ID for de-duplication
- **Events are versioned, typed JSON** (`type`, `id`, `occurred_at`, `data`), described
  in OpenAPI or JSON Schema so the TS client gets types for them.
- On Workers, dispatch fits Queues or a Cron trigger. On a VPS, a background task.

## 6. Spores (in-process extensions)

```rust
pub trait Spore {
    /// "spore_stripe": names its migrations module, tables, custom field namespace, scopes.
    fn name(&self) -> &'static str;
    fn migrations(&self) -> MigrationSet;
    fn routes(&self) -> Vec<SporeRoute>;            // e.g. POST /webhooks/stripe
    fn event_handlers(&self) -> Vec<EventHandler>;
    fn custom_fields(&self) -> Vec<FieldDefinition>;
    fn policies(&self) -> Vec<Box<dyn AnyPolicy>>;
    // plus the ports it provides, e.g. a PaymentProvider
}
```

- Spores are compiled into the host binary. There's no runtime plugin loading, which
  keeps types, `Send` proofs and WASM builds simple. WebAssembly components could allow
  loadable plugins later; that's a separate decision.
- **Spore storage kit:** spores write SQL the way `hypha-store-postgres` does. We share
  its pieces:
  - column helpers
  - error mapping by constraint name → `RepoError::Duplicate`
  - the version-checked update
  - `MigrationSet`
  - the contract-test harness

  There's **no ORM**: sqlx, SeaORM and Diesel don't run on Workers. If raw SQL gets
  painful, `sea-query` (a query builder with no runtime dependency) is the first thing
  to evaluate.
- **Outbound HTTP** (to Stripe, ERPs) goes through an `HttpClient` port, because native
  and Workers make requests differently. **Secrets** come from a per-store secrets port.

### Example: a Stripe spore (with checkout)

- **Core defines the port:**
  `trait PaymentProvider { create_payment, capture, refund, parse_webhook }`, with typed
  errors (`payment_declined`, ...).
- **The spore** calls Stripe's REST API through `HttpClient`. Its own tables
  (`spore_stripe_payments`, `spore_stripe_events`) are created by its `MigrationSet`.
- **Flow:**
  1. `start_checkout`: check the cart is fully available, then create a pending order
     with frozen prices.
  2. Ask the provider for a payment intent (Stripe idempotency key = order ID) and return
     the client secret to the storefront.
  3. Stripe calls `POST /webhooks/stripe`: verify the signature, then record the event
     ID under a unique key, so a repeated delivery is a harmless `Duplicate`.
  4. `confirm_payment` marks the order paid and records the `OrderPaid` event.
- **Tests:** a fake `PaymentProvider` in testkit, plus Stripe test mode.

## 7. What to reserve now, while building core commerce

These are cheap now and expensive to retrofit:

| Reserve | Why now | When |
| --- | --- | --- |
| Per-module migrations | Spore tables need their own version lists | **Done** (`MigrationSet`, 0001) |
| Aggregates record domain events; repos write them to an `outbox` in the same transaction | Retrofitting means touching every aggregate, repo and contract test | **With orders**, the first aggregate whose events matter externally |
| A policy check point in commands (after domain validation, before the write) | Adding it to every command later is tedious | When the first policy is needed; keep commands shaped "load → validate → write" so it slots in |
| API DTOs that can derive OpenAPI schemas | The generated TS client is the main developer experience | With the next routes; one derive per DTO |
| API keys with scopes → a real `Ctx` | External apps need them before anything else | Next, with users and auth |
| No foreign keys from extension tables into core | Keeps tiers and tenancy independent | Already the rule |

## 8. Not decided here

- Webhook delivery details: retry schedule, dead letters, per-app subscriptions.
- Rate limits and quotas per app.
- An admin UI extension model (embedded app pages, like Shopify).
- Loadable plugins (WASM components).
- Multi-tenancy (still open from 0001) affects namespacing of definitions, apps and
  webhooks.
