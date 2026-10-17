# 0025: Hosting, and the `hypha` CLI

- **Status:** Accepted (2026-10-17). VPS first; Cloudflare later, with its own host.
- **Scope:**
  - Docker images for the API, the admin and the storefront;
  - one server running all of it (Compose, Caddy for HTTPS);
  - `packages/cli`: `hypha init`, `deploy`, `status`, `logs`.
- **Not in scope:**
  - a native Cloudflare Workers host for the API (H2; a design note of its own);
  - managed databases and object stores as choices in `init` (they work through
    `.env`, untested);
  - backups, upgrades with rollbacks, several servers, zero-downtime deploys;
  - prebuilt images in a registry (H8 leaves room);
  - storefront and admin templates to choose from (H6 leaves room).
- **Related:** 0006 (one store per deployment), 0011 (the apps' stack), 0013 (media),
  0017 (configuration), 0023 (storefront), 0024 (caching).

## Decisions

| | Decision |
| --- | --- |
| H1 | **A VPS first.** One server with Docker runs everything. It's the cheapest real setup, it needs nothing Hypha doesn't have, and the same images serve every container platform later. *Decided* |
| H2 | **Cloudflare gets a native Workers host, later.** Not the axum image in Cloudflare Containers: they sleep when idle (stopping the timed jobs), and "the same core on Workers" is a goal (CLAUDE.md). The domain crates and the Postgres adapter already build for wasm; the host needs Hyperdrive, a Durable Object for rate limits, Cron triggers for jobs and `waitUntil` for background work. *Deferred* |
| H3 | **Everything in Docker.** The API, the admin, the storefront, Postgres, file storage (SeaweedFS, as in development) and Caddy, in one `compose.yaml`. The server needs only Docker; `deploy` offers to install it. *Decided* |
| H4 | **Caddy in front, the only thing exposed** (80, 443). It gets and renews the certificates (Let's Encrypt) and sends each domain to its service. *Decided* |
| H5 | **The apps run as Node servers on a VPS** (vinext's standalone output, `HYPHA_TARGET=node`), and as Workers on Cloudflare. Same code, same `vite build`; only the Cloudflare plugin is left out. *Decided* |
| H6 | **The built-in admin and storefront are optional.** `--no-admin` / `--no-storefront` leave them out (a Compose profile each), for a custom one pointed at the API. Templates to pick from come later. *Decided* |
| H7 | **The CLI is TypeScript** (`@clack/prompts`, `citty`), run by Node directly (type stripping, no build). Cloudflare's tools are Node, and the people deploying storefronts have it. What needs the core stays in the Rust binary (`config`, `openapi`); the CLI calls it, never copies it. *Decided* |
| H8 | **Images are built from the repository**, on the server over SSH (its own architecture, nothing to copy) or here and copied (`docker save \| docker load`) for small servers. Published images (GHCR) come later and slot in as a third source. *Decided* |
| H9 | **A deployment is a folder** (`hypha init` writes it, default `hypha-deploy/`, ignored by git): `hypha.json` (the CLI's choices), `.env` (secrets and addresses, mode 600), and the files the server runs. It's the source of truth: `deploy` copies it every time. *Decided* |
| H10 | **Every question has a flag** (`--yes` takes the defaults), so agents and scripts can run `init` unattended (CLAUDE.md, agent-friendly). *Decided* |

## 1. The stack

```text
          :80 / :443
              │
           caddy ─── api.<domain> ───► api:3000 ──► postgres:5432
              │  ├── admin.<domain> ─► admin:3000 ─┐
              │  ├── <domain> ───────► storefront:3000 ─┤ (HYPHA_API_URL=http://api:3000)
              │  └── media.<domain> ─► s3:8333          │
              └──────────────────────── api ◄───────────┘
```

- **Trusted proxies.** The network has a fixed subnet (`HYPHA_NETWORK`, default
  `172.30.250.0/24`), the API's `trusted_proxies`: Caddy and the apps reach it from
  there, and Caddy appends the browser's address to `X-Forwarded-For`. The admin passes
  it on (`ADMIN_TRUST_X_FORWARDED_FOR=true`) for the login limits (0009).
- **Files.** Upload URLs are signed for the host they name, so the API knows the
  storage by its public address (`HYPHA_S3_ENDPOINT=https://media.<domain>`), the one
  browsers use. Inside the network, Caddy carries that name as an alias, so the API's
  own calls stay on the server. Public files are read anonymously
  (`HYPHA_MEDIA_URL=…/hypha-media`): static pages keep plain URLs that don't expire.
- **Secrets** are made by `init`: Postgres's password, `HYPHA_SECRET_KEY`, the S3 keys
  and the first owner's password (shown after `deploy`, to change).

## 2. `hypha deploy`

1. Checks the server: SSH with a key, Docker with Compose (offers Docker's install
   script), enough memory to compile (offers a swap file; see §3), and DNS: every domain
   should lead to the server, or Caddy gets no certificate.
2. Builds the images (`hypha-api`, `hypha-admin`, `hypha-storefront`, tag `current`).
3. Copies the folder's files to the server (`/opt/hypha`) and runs
   `docker compose up -d`.
4. Waits for `https://api.<domain>/health` (the first time, Caddy is getting
   certificates).
5. The first time, connects the storefront: as the owner, a publishable key for the
   Online Store channel and the storefront as an app whose webhook is
   `https://<domain>/api/revalidate` (0024, §7). Both go into `.env`, and the storefront
   restarts with them.

Run again after any change; Compose restarts what changed. `--target local` runs the
same stack on this machine under `*.hypha.localhost`, over plain HTTP, to try it.

## 3. Sizes

Compiling the API peaks near **3 GB** of memory (its last crate, about two minutes on
an M-series laptop); the apps' builds under 1 GB. A 4 GB server (Hetzner CX22, CAX11)
builds with a little swap, which `deploy` offers to add. Running, the whole stack takes
about 425 MB (measured on a CX22 with the seeded store: S3 145, storefront 90, admin
70, Postgres 60, the API 35, Caddy 25).

## 4. Caching on a VPS

One Node process per app, so the in-memory cache vinext uses by default is enough: the
storefront's static pages and the webhook's revalidation (0024, §7), and the admin's
per-session reads (0024, §6), all live in that process. On Workers (H2) they need
Cloudflare's adapters (KV, the Workers Cache), and the admin's cross-request cache
should go: an isolate that didn't see a write would keep serving the old read.

## 6. Measured on the test server, and what changed

A CX22 (2 shared vCPUs, 4 GB) with the seeded store, measured with `pg_stat_statements`
(on in every deployment), the API's per-request logs and autocannon (50 connections
from Croatia to Germany). Before building the Workers host: on Workers, every API call
and every page render is an invocation billed by count and CPU time, and every query
is a round trip through Hyperdrive.

| | Before | After |
| --- | --- | --- |
| Product read (API): throughput | 275 req/s | 484 req/s |
| Search (API): throughput | 276 req/s | 501 req/s |
| Product read: Postgres planning / executing, per read | 4.5 ms / 1.2 ms | ~0 / 1.1 ms |
| Queries while idle | ~5 per second | ~8 per minute |
| Admin home: API calls / queries | 18 / 154 | 4 / 49 |
| Product editor: API calls | 3 + one per variant | 6 |
| Search page render (storefront CPU on the server, from cache) | 33 ms, 208 KB | 31 ms, 153 KB |

- **Postgres planned every query again** (unprepared statements: 80% of its time on a
  product read). Pooled connections now keep their prepared statements
  (`StatementCache`, `database.statement_cache`, on by default; off behind a pooler in
  transaction mode). The repositories run queries through `Queries` (`Db`): prepared
  when the connection keeps a cache, unprepared otherwise, so a Workers host (one
  connection per request, Hyperdrive's transaction pooling) runs the same code without.
- **A load's queries went one after another.** A product load (product, options,
  values, variants, media), a market, a location and an inventory item now select their
  children through the root's filter (a join) instead of waiting for its IDs, and send
  them together (`try_join`), pipelined in one round trip. (A read-only snapshot also
  ended by dropping its transaction, a queued ROLLBACK, instead of waiting for a COMMIT;
  through Hyperdrive that stalled the next query, so transactions end explicitly again:
  0026, §8.) Fewer round trips matter little on one server and a lot through Hyperdrive.
- **The market was read twice** for a priced product, and once per market again by
  search sync (`load_pricing_for` takes the market already loaded). **The publishable
  key** was looked up on every storefront request; it's cached for 30 seconds
  (`key_cache.rs`), and revoking a key clears the cache.
- **The jobs polled every 2 seconds**, which keeps a serverless database (Neon) awake
  around the clock. An idle job now doubles its wait up to `jobs.idle_interval` (a
  minute), a successful write wakes the dispatcher, and a dispatch step that did work
  wakes search sync, so changes still apply at once.
- **The admin asked per item**: the home page's checklist read six lists plus two reads
  per sampled product, and the product editor read each variant's stock.
  `GET /admin/setup/checklist` and `GET /admin/products/{id}/inventory` answer each in one
  call. The admin's cache already makes a repeated page cost one call (`/admin/me`).
- **The storefront's dynamic pages rendered their skeletons every time.** A route's
  `loading.tsx` is rendered (server components and HTML) before its page even when the
  page's data comes from cache at once; Astryx's `Skeleton` carries about fifteen atomic
  classes per element, and the search skeleton has a hundred elements. They're now one
  element with one class each (`.shop-bone`), the same look: a quarter less HTML, but
  only 7% less CPU on the server (a laptop's numbers suggested half; measure on the
  server, from inside the container, before believing a render optimisation).

What's left, for the Workers host's note:

- **Dynamic pages are rendered per request**: on the CX22, search costs ~31 ms of CPU
  (about 38 a second on one core), the cart page ~16 ms (mostly the header and footer),
  a cached product page ~4 ms. That's the framework rendering an Astryx tree twice
  (server components, then HTML), with no single hotspot in the profile. Search and cart pages hold nothing about the
  shopper (the cart comes in the browser), so their HTML could be cached by URL at the
  edge for a minute; only checkout, order and account must stay private. On a VPS that
  needs a cache in front of Caddy; on Workers it's the Workers Cache.
- **Static pages** (product, category, home) cost under 1 ms from vinext's cache, which
  is per process: on Workers they need the KV or Workers Cache adapters so every isolate
  shares them and the webhook's revalidation reaches them all.
- **Logging in costs Argon2id** (tens of milliseconds of CPU natively, more in wasm):
  over the free plan's 10 ms per request, fine on the paid plan.
- **Astryx's markup is heavy** (about 11 classes per element, a third of a page's HTML);
  a leaner product card would cut the listing's render further, at the cost of the
  storefront being built from Astryx components only.

## 7. Later

- Published images, so `npx @hypha/cli` works without the repository (H8).
- `hypha backup` / `restore` (Postgres and the files), `hypha upgrade` with a rollback.
- The Cloudflare host (H2), then `--target cloudflare`.
- Storefront and admin templates in `init` (H6).
