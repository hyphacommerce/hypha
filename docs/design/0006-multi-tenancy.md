# 0006: Multi-tenancy

- **Status:** Accepted (2026-10-13). T1–T4 are decided as recommended (section 4).
- **Question:** how does one Hypha deployment relate to stores (merchants)? This affects
  every table, how staff, API keys, customers and orders belong to a store, and how the
  Cloudflare host scales. It's cheap to decide before users, customers and orders exist,
  and expensive after.
- **Related:** 0001 (D1's 10 GB per database and 50,000 databases per account; Hyperdrive
  is configured per database), 0002 (external apps, API keys and webhooks are per store).

## 1. Who deploys Hypha

The primary users are TypeScript developers building **client stores** (0002). In
practice that means:

- **Agencies and freelancers**, who deploy and run one store per client. Different
  clients' data must never mix. Each client may want a separate bill and region.
- **Later, maybe a hosted Hypha** (SaaS): one operator runs many stores on shared
  infrastructure.
- **One merchant with several brands or stores** (separate catalogs, domains,
  checkouts). Distinct from *markets*, which are countries or currencies inside one store.

## 2. The three classic models

| | A. One store per deployment | B. Shared tables, `store_id` on every row | C. One database (or schema) per store |
| --- | --- | --- | --- |
| Isolation | Physical: separate database | Logical: every query must filter by store. **One forgotten filter leaks another store's data**, the classic SaaS breach. Postgres row-level security reduces the risk | Physical |
| Unique keys (handle, country, …) | As today | Must become `(store_id, handle)` etc. | As today |
| Migrations | One database | One run for all stores | One run **per store** (N runs, N times things can fail) |
| Connections | One pool | One pool | A pool per store; on Workers, a Hyperdrive config per database |
| Cloudflare D1 | One D1 database | Hits D1's 10 GB cap with many stores | **D1's intended model** (50,000 databases per account) |
| Many tiny stores (SaaS) | Expensive: one deployment each | Cheapest | Fine on D1; heavy on Postgres |
| Cross-store reporting | Separate systems | Easy (one query) | Hard |
| Fits agencies (one store per client) | **Best** | Possible | Possible |
| Complexity now | **None** | High: every table, query, test and key changes | Medium: routing, per-store migrations |

None of them is right for every user. Agencies want A, a hosted Hypha wants B or C, and
D1 wants C.

## 3. Proposal: tenancy belongs to the ports, not the domain

**Aggregates, commands and domain crates never see a store.** `Product` has no
`store_id`, and `create_simple_product` doesn't filter by store. Instead, the host
resolves *which store* a request is for, and asks the storage for **ports scoped to that
store**:

```rust
// host (sketch)
let store = resolve_store(&request)?;          // from the API key, later maybe the hostname
let ports = state.storage.ports(store).await?; // repos that only see that store's data
create_simple_product(&ctx, ports.products(), &state.ids, input).await?;
```

Then A, B and C are **adapter choices**, invisible to the domain:

- **A:** `ports(store)` ignores `store`. One database is one store.
- **B:** `ports(store)` returns repos that add `store_id = $store` to every query
  (and row-level security as a safety net).
- **C:** `ports(store)` looks up that store's database (pool, Hyperdrive binding or D1
  binding) and returns repos on it.

Why this works for Hypha in particular:
- Every ID is already a globally unique UUIDv7, so a store's data can move between
  models (export from shared tables into its own database) without renumbering.
- Unique keys are enforced by the repos (0001), so scoping them per store is a storage
  detail as well.
- The contract tests (0001, step 3) already run every scenario on a fresh repo. "Two
  stores don't see each other's data" becomes one more contract scenario for B and C.

### What to build now

- **`StoreId`** (prefix `store_`) in core, and **`Storage::ports(store: StoreId)`** in
  the host. Today the host has exactly one store and the adapters ignore it. That's
  model A, at almost no cost.
- **API keys resolve to `(store, scopes)`** when they arrive with users and auth (next
  step). In model A, every key belongs to the one store.
- **A `Store` record** (name, default currency, settings) is created in the store's own
  database. It's the natural owner of store-wide settings later (policies and custom
  field definitions from 0002).

### What to build later, when someone needs it

- **C on Cloudflare:** one D1 database (or Postgres via Hyperdrive) per store, plus a
  small **control plane** that maps hostnames and API keys to stores and runs migrations
  for each store.
- **B for a hosted Hypha on Postgres:** a `store_id` column added *by the adapter's
  migrations*, composite unique keys, row-level security, and the cross-store contract
  scenario. Existing single-store databases get their one store's ID filled in.

## 4. Decisions

- **Decided: T1, default deployment model:** **A** (one store per deployment). Fits
  agencies, the primary audience, and costs nothing now.
- **Decided: T2, where tenancy lives:** **only in hosts and adapters**, never in
  aggregates or commands, as in section 3. The alternative is `store_id`
  on every aggregate: simpler to grasp, but it puts every leak risk in domain code, and
  it rules out a database per store without a rewrite.
- **Decided: T3, reserve now:** `StoreId` and `Storage::ports(store)`, with a single
  store.
- **Decided: T4, where staff users and API keys live:** in the store's database, for
  model A. A control plane holds them only if C is built, since that's when one login
  can span several stores.

## 5. Not decided here

- Agency features: one login across several client stores, cross-store dashboards.
  These belong to a control plane (C) or to external apps.
- Store-to-store data sharing (one catalog shared by several brands).
- Data residency (EU-only stores). It fits C naturally, since each store's database can
  live in its own region.
- Hostname routing (`shop.example.com` → store) versus API-key routing only.
