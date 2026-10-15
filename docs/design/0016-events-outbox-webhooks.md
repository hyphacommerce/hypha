# 0016: Domain events, the outbox, jobs and webhooks

- **Status:** Accepted (2026-10-15). The direction in the decisions table was agreed
  after the October audit (the event path, the jobs seam, one `App` record, encoded
  cursors and extensible enums); Danilo accepted every recommendation in section 11 the
  same day, marked *Decided*.
- **Scope:** how commands record domain events, how they reach the outbox in the same
  transaction as the change, how a host runs background work on every runtime, how
  events are delivered (in-process handlers and webhooks to external apps), the `App`
  record webhooks belong to, the first event catalog and the first handler (cleaning up
  after a removed variant, 0014 §4).
- **Not in scope:** fulfilment (0017), search (0018; it consumes these events), spores
  (their handlers plug into the dispatcher later, section 6), emails through the outbox
  (section 10), OAuth for apps and open scopes (0008 §9, section 10), a public event feed
  for polling (section 10).
- **Related:** 0001 (storage, batches), 0002 §5 and §7 (events and the outbox were
  reserved there), 0007 §10 (orders before payments and the outbox), 0008 (secrets,
  `SecretBox`), 0013 (the media sweep, the first background job), 0014 §4 (removed
  variants), 0015 §5 (the signed callback webhooks reuse) and §9 (no connection held over
  outbound calls).

## Decisions

| | Decision |
| --- | --- |
| EV1 | **Aggregates record events; every write path stores them.** An aggregate's methods append to a pending list. `insert`/`update` and batch staging take the pending events, and the adapter writes them to the outbox **in the same transaction** as the aggregate. An event exists if and only if its change committed. Commands mostly don't change. *Hard to reverse* |
| EV2 | **Every repository write is a batch of one inside the adapter.** Each aggregate gets `insert_x`/`update_x(tx, …)` functions; the repo methods and the batch both call them. Batches become one ordered list of staged writes, so a new aggregate (or the outbox) is one variant, not nine edits |
| EV3 | **Set-based writes go through batches** with a `StageEvents` step: catalog membership, fixed prices, tax-class assignment and `update_many` today skip aggregates, so their commands stage the write and its event together |
| EV4 | **Event payloads are contract types in their domain crate** (`events.rs`), plain serializable data, never the aggregate. Each has a stable dotted name (`product.variant_removed`) and a version. *Hard to reverse* |
| EV5 | **The envelope** is `{id: "evt_…", type, version, subject: {type, id}, actor, occurred_at, data}`. `occurred_at` is the writing transaction's time. Every event carries the `Actor` that caused it, so agent actions are audited for free |
| EV6 | **The outbox orders by a `bigint` identity, not by UUIDv7**, and is read by claiming undelivered rows (`FOR UPDATE SKIP LOCKED`), never through an "after this ID" cursor: IDs aren't in commit order, so a cursor would skip events from transactions that commit late |
| EV7 | **Delivery is at least once, with no ordering guarantee.** Each event carries its subject's version when it has one; consumers use it to drop stale ones. Handlers must be idempotent (section 6) |
| EV8 | **One delivery row per event and destination**, with attempts, the next attempt time and the last error, retried with backoff. In-process handlers and webhooks are both destinations, so they retry, fail and show up the same way |
| EV9 | **No connection is held while a destination runs.** The dispatcher leases a few deliveries in a short transaction, releases the connection, runs them, then records the outcomes in another short transaction (0015 §9's lesson) |
| EV10 | **A jobs seam, not `Storage` methods.** Background work is plain functions, `run_due(ports, clock, …) -> Progress`, that a host calls from a Tokio interval, a Workers Cron trigger or EventBridge. The host owns one runner that starts them and drains them on shutdown (section 7) |
| EV11 | **One `App` record** (`app_<name>`) owns an external app's name, signing secret, webhook endpoint and event subscriptions. Shipping's app providers keep only their rate callback (URL, timeout, services) and use the app's secret. OAuth and app scopes join the same record later (section 8). *Hard to reverse* |
| EV12 | **Webhooks are signed like the rate callback** (`Hypha-Signature: t=…,v1=…`, 0015 §5), sent only to public addresses (`outbound.rs`), and carry the event envelope as the body |
| EV13 | **Public contract fixes before the first webhook consumer:** list cursors are encoded (not the last item's raw ID) and closed enums in the API documents are marked extensible, so new states (0017) don't break generated clients (section 9) |

## 1. What others do

- **Shopify** delivers webhooks at least once and out of order, with an event ID header
  for de-duplication. It retries failed deliveries for a while and removes subscriptions
  that keep failing. Payloads are the full resource.
- **Stripe** stores every event (`evt_…`), lets you list them, retries webhooks for days
  with backoff, and signs them (`t=…,v1=…`, the scheme 0015 already uses). Payloads embed
  the object as it was at the event.
- **Medusa v2** has an event bus with subscribers in-process; its outbox needs a Redis or
  similar module for durability.
- **The transactional outbox pattern** in general: write the event row in the same
  transaction as the state change, then deliver from that table. It's the only way to
  get "event if and only if committed" without two-phase commits.

Hypha takes the outbox pattern and Stripe's envelope and signature, and Shopify's
at-least-once, unordered delivery.

## 2. Recording events (EV1, EV4)

```rust
// hypha-core/src/events.rs
/// An event an aggregate recorded, serialized when recorded; the adapter turns it into
/// an outbox row (ID, actor, commit time) when the write commits.
pub struct PendingEvent {
    pub event_type: &'static str,   // "product.variant_removed"
    pub version: u16,               // 1
    pub subject: Subject,           // ("product", uuid)
    /// The aggregate's version after this write; `None` for set-based writes (catalog
    /// membership, fixed prices), which have no version of their own.
    pub subject_version: Option<Version>,
    pub data: serde_json::Value,
}

/// A domain event: a contract type with a stable name and version.
pub trait Event: serde::Serialize {
    const TYPE: &'static str;
    const VERSION: u16;
    fn subject(&self) -> Subject;
}

/// What an aggregate keeps its pending events in. Compares as always equal, so
/// `restore(snapshot) == aggregate` round trips still hold.
#[derive(Debug, Clone, Default)]
pub struct Recorder(Vec<PendingEvent>);
```

- An aggregate method that changes something calls `self.events.record(&VariantRemoved
  { product, variant })`. Commands don't record events themselves, except for
  set-based writes (section 3).
- Payload structs live in the domain crate (`hypha-products/src/events.rs`), with IDs as
  prefixed strings and `Money` as everywhere else. They're the webhook contract, so
  renaming a field is a new version (`version: 2`), never an edit.
- `hypha-core` gains `serde_json` as a plain dependency (today it's behind `openapi`);
  it builds for wasm.
- **The actor** comes from the ports: the host builds a request's ports in
  `Caller::authenticate`, the one place that also builds its `Ctx`, and the ports stamp
  every event they write with that actor. Jobs use `Actor::System`. (Q1, *Decided*.)
- **Event IDs and times come from storage:** the outbox row's `id` is
  `gen_random_uuid()` and its `occurred_at` is the writing transaction's time (`now()`), so
  aggregates need neither an ID generator nor a clock to record. Business times that
  matter (`placed_at`) are in the payload.

## 3. Writing events (EV2, EV3)

**The outbox table** (core migration):

```sql
CREATE TABLE outbox_events (
    seq             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    id              uuid        NOT NULL UNIQUE DEFAULT gen_random_uuid(),
    type            text        NOT NULL,
    version         smallint    NOT NULL,
    subject_type    text        NOT NULL,
    subject_id      uuid        NOT NULL,
    subject_version bigint,
    actor_kind      text        NOT NULL,
    actor_id        uuid        NOT NULL,
    data            jsonb       NOT NULL,
    occurred_at     timestamptz NOT NULL DEFAULT now(),
    fanned_out_at   timestamptz
);
CREATE INDEX outbox_events_pending ON outbox_events (seq) WHERE fanned_out_at IS NULL;
```

**Every write is a batch of one (EV2).** Today about half the aggregates write through
SQL inline in their repo methods and can't be staged. Each aggregate gets
`insert_x(tx, &Snapshot, &[PendingEvent])` / `update_x(…)` functions; `ProductRepo::update`
opens a transaction and calls `update_product` plus `insert_events`, and `PgBatch`
calls the same functions. The batch becomes:

```rust
pub struct PgBatch { staged: Vec<Staged> }
enum Staged {
    NewOrder(OrderSnapshot), Cart(CartSnapshot), Product(ProductSnapshot), …,
    Events(Vec<PendingEvent>),
}
```

applied in a fixed order (by kind, then key) so two batches lock rows in the same order;
inventory last, so its row locks are held for the shortest time. `InMemoryBatch`
mirrors it. Adding the outbox is the `Events` variant.

**Set-based writes (EV3).** `add_products_to_catalog`, `set_fixed_prices`,
`remove_fixed_prices`, `assign_tax_class` and the inventory `update_many` stage their
write (`StageCatalogProducts`, `StageFixedPrices`, …) and their event
(`StageEvents::record`) in one batch.

**In memory,** each repo holds a handle to an `InMemoryOutbox`; `InMemoryPorts` shares
one between all of them, and tests read it (`ports.outbox.events()`).

**Contract scenarios:** an event exists if and only if its write committed (a stale
update records none); a batch's events commit with it; a batch of one writes the
aggregate's events; claiming skips rows another claim holds.

## 4. The event catalog, version 1

Small payloads: the subject and the facts of the change, not the whole resource. A
consumer that needs the resource fetches it (Q2, *Decided*).

Every payload names its subject (`product`, `catalog`, …) as a prefixed ID. As built
(2026-10-15):

| Event | Data | Recorded by |
| --- | --- | --- |
| `product.created` | `handle`, `status` | creating a product |
| `product.updated` | `fields` changed (`title`, `handle`, `status`, `options`, `media`) | product edits, one per write |
| `product.variant_added` / `…_removed` | `variant` | variant edits and `set_options` |
| `product.variant_updated` | `variant`, `fields` (`sku`, `price`, `selections`, `weight`, `requires_shipping`, `media`) | one per variant per write |
| `catalog.changed` | (subject) | created, price list assigned |
| `catalog.products_added` / `…_removed` | `products` | catalog membership (staged, EV3) |
| `price_list.changed` | (subject) | created, adjustment changed |
| `price_list.prices_set` / `…_removed` | `variants` | fixed prices (staged) |
| `market.changed` | (subject) | created, countries or tax setting changed |
| `tax_class.changed`, `tax_region.changed` | (subject) | created; a region's rates changed |
| `product.tax_class_set` | `tax_class` (`null`: the default rate) | tax-class assignment (staged), one per product |
| `shipping_zone.changed` / `shipping_zone.deleted` | (subject) | any zone edit; deleting it |
| `inventory_item.changed` | `variant`, `tracked`, `continue_selling`, `available` per location | counts, adjustments, reservations, settings; built from the item as written |
| `order.placed` | `number`, `customer`, `total` | checkout |
| `order.paid` | `amount` | `mark_order_paid` (a payment `reference` comes with payment spores, as a version 2) |
| `customer.created` / `customer.updated` | `fields` (names only: `email`, `first_name`, `last_name`, `phone`, `addresses`, `defaults`) | customers, checkout's new guest |

Customer payloads never carry the values: an email or address is personal data, and a
consumer with `customers:read` fetches it. An edit that changes nothing records nothing.
"Changed" events are recorded once per write (`Recorder::record_once`).

Not events: cart edits (noise; abandoned carts come with a cart age), staff and auth
changes (an audit log is a separate thing; sessions and keys mustn't leak into
webhooks), media upload progress.

`inventory_item.changed` fires on every checkout. That's the busiest event; consumers
(search) take the latest per variant, and the subject version makes that safe.

## 5. Fan-out and delivery (EV6–EV9)

```sql
CREATE TABLE event_deliveries (
    id              uuid        PRIMARY KEY,
    event_seq       bigint      NOT NULL REFERENCES outbox_events (seq) ON DELETE CASCADE,
    destination     text        NOT NULL,  -- "handler:variant_cleanup", "app:app_erp"
    status          text        NOT NULL CHECK (status IN ('pending', 'succeeded', 'failed')),
    attempts        integer     NOT NULL DEFAULT 0,
    next_attempt_at timestamptz NOT NULL,
    leased_until    timestamptz,
    last_error      text,
    UNIQUE (event_seq, destination)
);
CREATE INDEX event_deliveries_due ON event_deliveries (next_attempt_at)
    WHERE status = 'pending';
```

Two job functions, each in short transactions:

1. **`fan_out`**: claim up to N events with `fanned_out_at IS NULL`
   (`FOR UPDATE SKIP LOCKED`, in `seq` order), insert one delivery per destination that
   wants the event type (the built-in handlers that subscribe to it, and each enabled
   app subscribed to it), set `fanned_out_at`. One transaction.
2. **`deliver_due`**:
   - **Lease:** in one short transaction, take up to N pending deliveries whose
     `next_attempt_at` has passed and whose lease has expired (`SKIP LOCKED`), set
     `leased_until = now + 1 minute`, commit.
   - **Run** each one with no connection held: a handler gets its own ports for its
     write, a webhook goes over HTTP.
   - **Record** the outcomes in another short transaction. Success sets `succeeded`.
     Failure increments `attempts`, stores the error and sets `next_attempt_at` from the
     schedule. After the last attempt it's `failed`.

A worker that dies mid-run leaves its lease to expire, and another picks the deliveries
up: that's the "at least once". The `UNIQUE (event_seq, destination)` makes fan-out safe
to repeat.

**Retry schedule** (Q6): after 1 min, 5 min, 30 min, 2 h, 6 h, then every 12 h up to 3
days in all, then `failed`. A failed delivery can be retried from the admin.

**Retention** (Q7): events and their deliveries are deleted 30 days after the event; a
job does it (section 7).

## 6. In-process handlers, and the first one

A handler is a destination in code:

```rust
pub struct Handler {
    pub name: &'static str,                 // "variant_cleanup"
    pub events: &'static [&'static str],    // ["product.variant_removed"]
}
```

and the dispatcher matches a delivery's handler name to a function. Handlers live in a
new **workflow crate, `hypha-dispatch`**, because they compose domains (the cleanup
touches pricing and inventory), like checkout does. Spores add theirs later through the
same table.

Handlers must be idempotent: a lease can expire mid-run and the delivery runs again.
Each handler's write is its own command, run as `Actor::System`.

**The first handler, `variant_cleanup`** (0014 §4), on `product.variant_removed`:
- removes the variant's fixed prices from every price list
  (`PriceListRepo::remove_variant_everywhere`, one statement)
- deletes the variant's inventory item if it has no reservations; with reservations it
  stays, since open orders still point at them, and 0017's fulfilment decides when it
  goes (Q5)

Both records `price_list.prices_removed` / `inventory_item.changed` events of their
own, so search hears about them.

## 7. The jobs seam (EV10)

```rust
// hypha-dispatch (domain-level, no runtime): plain functions over ports.
pub async fn fan_out<S: DispatchPorts>(ports: &S, clock: &impl Clock, limit: u32) -> Result<Progress, RepoError>;
pub async fn deliver_due<S: DispatchPorts, W: WebhookSender>(…) -> Result<Progress, RepoError>;
pub async fn prune_events<S: …>(…), prune_idempotency_keys<S: …>(…), prune_sessions<S: …>(…);
```

`Progress { done: u32, more: bool }` lets a runner loop while there's more and sleep
when there isn't.

**The host side** (`hypha-host-axum/src/jobs.rs`): one runner with named jobs, each
with an interval (`fan_out`/`deliver_due` every 2 s, pruning hourly, the existing media
sweep hourly, the rate limiter's idle sweep every minute). It spawns them with a shared
shutdown signal and drains them on shutdown, replacing `Storage::spawn_media_sweep` and
the loose `tokio::spawn`s in `main.rs`. `Background` stays for work a request starts
(password-reset emails) until those move to the outbox.

As built: `Storage` keeps one method, `start_jobs`, since spawning needs a concrete
storage to prove `Send` (the reason `spawn_media_sweep` existed); the jobs are
`dispatch` (every 2 s: fan out, then lease, run each delivery with its own ports, and
record), `prune` (hourly: events after 30 days, idempotency keys after a day, sessions
30 days after they ended, through `IdempotencyRepo::prune` and `SessionRepo::prune`),
`media_sweep` and `rate_limits`. Shutdown stops them after their current step, waiting
up to 10 seconds.

**Workers** call the same functions from a Cron trigger (once a minute, so delivery
latency there is up to a minute; Queues can shorten it later). Lambda: EventBridge.

## 8. Apps and webhooks (EV11, EV12)

**A new leaf crate, `hypha-apps`:**

```rust
pub struct App {
    key: AppKey,                       // "app_erp", like shipping's provider keys
    version: Version,
    name: String,
    enabled: bool,
    secret: Sealed,                    // signs webhooks and rate callbacks
    webhook: Option<WebhookEndpoint>,  // URL (https, public), timeout
    subscriptions: Vec<String>,        // event types, or "*"; validated against the catalog
}
```

- **Shipping moves onto it.** `shipping_app_providers` keeps the callback URL, timeout and
  services. Name, `enabled` and the secret move to `apps`: a migration copies them, and the
  host's rate client reads the secret from the app. Shipping never depends on
  `hypha-apps` (leaf to leaf); it stores the key, and the host joins them, like other IDs
  across domains.
- **Routes** (`apps:read` / `apps:write`, new scopes):
  - `POST/GET /admin/apps`, `GET/PATCH/DELETE /admin/apps/{key}`
  - `POST /admin/apps/{key}/secret` (rotate)
  - `GET /admin/apps/{key}/deliveries` (newest first, filter by status)
  - `POST /admin/apps/{key}/deliveries/{id}/retry`
  - `GET /admin/events/{id}` (one event, for debugging a delivery)

  The shipping provider routes stay for the rate settings.
- **A delivery** is `POST <webhook url>` with the envelope as the body and these headers:
  - `Hypha-Signature`
  - `Hypha-Event-Id`
  - `Hypha-Event-Type`
  - `Hypha-Delivery-Id` (stable across retries)

  Any 2xx within the timeout (default 10 s) counts as success. The answer is read up to
  the same 256 KiB cap and ignored.
- **The sender is a port, `WebhookSender`**, implemented by the host over
  `outbound::client`, so `hypha-dispatch` stays runtime-free and the SSRF guard applies.
- **The admin** gets Settings → Apps: create an app, set its webhook and subscriptions,
  see deliveries and retry failed ones. Shipping's provider list links to the app.

As built (2026-10-16):

- `App` keeps its subscriptions as patterns (`order.placed`, `product.*`, `*`), checked
  for shape but not against the catalog, since `hypha-apps` is a leaf and can't see the
  other domains' event types. An unknown type simply never matches. `App::wants` is true
  for an enabled app with a webhook and a matching pattern. Fan-out asks it as the apps
  are at that moment.
- **Storage links shipping to apps.** `0029_apps.sql` moves name, `enabled` and the secret
  (unchanged, so `hypha_shs_` secrets keep verifying) into `apps`. A foreign key
  `shipping_provider_app` makes a provider need its app (`provider_app_not_found`, 422)
  and go when the app is deleted. The host's `HostRates` reads both. A disabled app
  quotes no rates.
- **The envelope** adds the subject's version (`subject: {type, id, version}`, EV7). The
  actor is `{type, id}`, with a null `id` for the system. Subject IDs are prefixed for the
  catalog's kinds; any other kind (a spore's) keeps its bare UUID. `GET /admin/events/{id}`
  returns the same envelope.
- **A delivery to an app without a webhook fails for good at once**, rather than being
  retried for 3 days; the admin can retry it once the webhook is back. A disabled app's
  deliveries wait on the retry schedule instead, so pausing an app keeps its backlog.
  Deleting an app deletes its deliveries too, so an app made later under the same key
  starts clean.
- **Leases bound a run.** A delivery is leased for 120 s and carries its lease. A runner
  starts one only with at least 45 s of it left (`has_time`: the longest webhook timeout,
  30 s, and room), and `record_outcomes` skips an outcome whose lease isn't current any
  more. So a slow batch never sends past its lease, and an expired lease can be taken or
  retried safely. The host loads the app with a pooled connection and lets it go before
  sending (`prepare`, then `send`; EV9).
- **Retry** (`POST …/deliveries/{id}/retry`) makes a delivery due now and keeps its
  attempts. A failed one therefore gets exactly one more try. A delivery that's leased
  (running) is left alone. Deliveries are listed by their event's `seq`, unique per
  destination, since delivery IDs are random v4. The page cursor carries that `seq`.
- Event and delivery IDs are `evt_…` and `dlv_…` (`EventId`, `DeliveryId` in core). A
  key in a URL that can't be an app key is `404 app_not_found`, like a missing app.

## 9. Contract fixes that come first (EV13)

Before any external consumer reads events or lists:

- **Cursors** become `base64url(json { v: 1, after: <last id>, filters })`. A cursor
  used with different filters is `400 invalid_cursor`. Clients already treat it as
  opaque (0012), so this changes no code that follows the docs.
- **Enums that will grow** are documented as open. Every named enum schema
  (`FinancialStatusDto`, `FulfillmentStatusDto`, `Currency`, `Scope`, …) and every inline
  enum with several values is affected. Single-value inline enums are tagged unions'
  tags and stay literal; request-only enums (`CLOSED_ENUMS` in `openapi.rs`) stay closed.
  - The documents mark them `x-hypha-open-enum: true` and list the known values in the
    description.
  - The SDK generates them as `string`, so a new value is never a compile break and
    clients have to handle one they don't know.
  - We tried `"a" | "b" | (string & {})` first, which keeps editor suggestions.
    openapi-fetch's `Readable<T>` maps over `string & {}` as if it were an object, so
    response types came out mangled.

## 10. Later, and what's reserved for it

- **Emails through the outbox.** Invites and password resets would become
  `staff.invited` events with an email handler, retried like any delivery. The reset
  token must not sit in the outbox in plain text: seal it with `SecretBox` first, or keep
  emails on `Background`. Its own change.
- **A public event feed** (`GET /admin/events?after=`) for polling consumers needs a
  safe horizon, because rows appear out of commit order. Postgres's
  `pg_snapshot_xmin(pg_current_snapshot())` gives one. Not needed for webhooks.
- **Spore handlers** register through the same `Handler` table; spore tables staged in
  the same batch need an extension slot in `Staged` (a boxed spore write).
- **OAuth apps and open scopes:** `App` gains granted scopes and API keys belonging to
  it. `Scope` becomes an open, registered type before spores name their own (stored
  scopes the binary doesn't know must not fail a role's load).
- **Endpoint health:** automatically disabling an app's webhook after days of failures,
  with an email to the owner.

## 11. Open decisions

**Q1. Where events get their actor.**
- A. *(Decided)* From the ports. The host builds ports and `Ctx` together in
  `Caller::authenticate`, so the ports stamp every event with that actor. Nothing at the
  call sites changes. Tests and jobs get `Actor::System` unless a test sets another.
- B. Every write takes the `Ctx` (`repo.update(&mut product, ctx)`). Explicit, but ~80
  call sites, every repo trait and the contract suite change, and nothing stops passing
  the wrong `Ctx` either.

**Q2. How much an event carries.**
- A. *(Decided)* The subject and the facts of the change. Consumers fetch the
  resource when they need it. Payload versions stay rare, and nothing about a resource
  is sent to an app that subscribed to its events but lacks the scope to read it.
- B. The whole resource as it was (Stripe, Shopify). Convenient for consumers, but
  every response shape change becomes an event version too, and payloads get big
  (products with 2048 variants).

**Q3. Timestamps on aggregates in this note.** `created_at` / `updated_at` on every core
table, read back into snapshots and shown in responses. As built: the adapter stamps an
aggregate from the clock its ports carry when it writes or stages it (like the version),
not the database's `now()`, so a batch's aggregates have their stamps before it commits
and responses always show them; the columns' `DEFAULT now()` only fills rows written by
hand. Tests stamp with a fixed clock. Each write sets them with one more statement in
its transaction, to fold into the main INSERT/UPDATE later with the other double writes.
- *(Decided)* Yes, as its own step here: every write path is touched for the events
  anyway, search wants `updated_at`, and 0001 §3.5 already promised it.

**Q4. Moving shipping's app providers onto `App` now** (EV11).
- *(Decided)* Yes. Nothing is deployed yet, so the migration is cheap now and costly
  once merchants have apps.

**Q5. Inventory items of removed variants that still have reservations.**
- *(Decided)* Keep them until fulfilment (0017) consumes the reservations, then
  delete them there.
- The alternative, deleting them anyway, loses the reservations open orders rely on.

**Q6. Retry schedule:** about 3 days of backoff, as in section 5. Shopify retries for
about 4 hours, Stripe for 3 days. *(Decided: 3 days.)*

**Q7. Retention:** 30 days for events and deliveries. *(Decided.)* Shorter keeps
the tables small; longer helps debugging.

**Q8. Built in or a spore.** The dispatcher, handlers and webhooks are core (`hypha-dispatch`,
`hypha-apps`), not a spore. *(Decided: core, since spores themselves need events.)*

## 12. Order of work

1. **Contract fixes** (section 9): encoded cursors, open enums. Small, separate.
2. **Preparation refactors**, no behaviour change, each its own commit:
   - core `retry_on_conflict` and idempotency replay helpers
   - per-aggregate `insert_x`/`update_x` with every repo write as a batch of one
   - `Staged` lists in both batches
   - named `try_get` row decoding
3. **Events:** core types, the outbox table and contract scenarios, recording in
   products first, then the other aggregates. Set-based writes move to batches.
   Timestamps if Q3 is yes.
4. **Jobs and dispatch:** `hypha-dispatch` with `fan_out`, `deliver_due`, pruning and
   `variant_cleanup`; the host's job runner replacing the ad-hoc spawns.
5. **Apps and webhooks:** `hypha-apps`, the shipping migration, routes, the sender, the
   admin's Settings → Apps. *(Built.)*
6. **e2e** steps for every new route; a webhook delivered to a local receiver
   (`HYPHA_ALLOW_PRIVATE_CALLBACKS=true` in e2e). *(Built, with step 5.)*
