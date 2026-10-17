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

## 5. Later

- Published images, so `npx @hypha/cli` works without the repository (H8).
- `hypha backup` / `restore` (Postgres and the files), `hypha upgrade` with a rollback.
- The Cloudflare host (H2), then `--target cloudflare`.
- Storefront and admin templates in `init` (H6).
