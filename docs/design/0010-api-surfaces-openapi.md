# 0010: Admin and storefront APIs, and OpenAPI

- **Status:** Accepted (2026-10-14). Danilo chose the recommended option for each of the
  hard-to-reverse decisions (S1 prefixes, S2 no version yet, S3 strict credentials per
  surface, S6 committed spec files with a freshness test); the rest are as recommended.
- **Scope:** splitting the HTTP API into an admin surface and a storefront surface, who
  may call each, which routes go where, and an OpenAPI document per surface.
- **Not in scope:** new endpoints (product and order listing, storefront markets), customer
  accounts (0008 §9), a docs UI beyond serving the JSON, MCP tool generation (later; this
  note keeps it possible).
- **Related:** 0008 (sessions, keys, `Scope::STOREFRONT`), 0009 (rate limits on the
  public auth routes), 0002 (TS-first external apps), CLAUDE.md (one command surface for
  HTTP, MCP and CLI; UCP for agentic shopping).

## Decisions

| | Decision |
| --- | --- |
| S1 | **Two path prefixes: `/admin/*` and `/storefront/*`.** Not Medusa's `/store/*`: in Hypha a "store" is the merchant (`Store`, `GET /store` today), and `/store/store` would be confusing. *Hard to reverse* |
| S2 | **No version in the path yet** (`/admin/products`, not `/v1/admin/products`). Nothing is released; when it is, a version goes in front (`/v1/…`) or in a header (Shopify dates its API versions). *Hard to reverse* |
| S3 | **Each surface says who may call it.** `/admin/*`: a staff session or a secret key; a publishable key is refused (`403 admin_credentials_required`) even if its scopes would allow the route. `/storefront/*`: a publishable key (later also a customer session); anything else is refused (`403 publishable_key_required`). *Hard to reverse* |
| S4 | **Staff auth moves under `/admin/auth/*`**, and `/me` to `/admin/me`. Customer auth will be `/storefront/auth/*`. `/health` stays at the root |
| S5 | **The storefront gets its own read models**, not the admin's. A storefront product is an active product (draft or archived is 404) without admin-only fields. This closes 0008's gap where a publishable key could read drafts |
| S6 | **One OpenAPI 3.1 document per surface**, built with `utoipa` 6 from the handlers and DTOs, served at `/admin/openapi.json` and `/storefront/openapi.json` (public: Hypha is open source), and committed as `openapi/admin.json` and `openapi/storefront.json`. A test fails when the committed files are out of date, so every API change shows up as a diff and TS clients can be generated from the files. *Hard to reverse: once clients generate from it, schema names and operation IDs are API* |
| S7 | **Routes are listed once.** The `router!` macro takes a table of `(method, path, handler)` per surface; the same table drives a test that every route is documented and every documented operation is routed |
| S8 | **Core types document themselves.** An optional `openapi` feature on `hypha-core` implements `ToSchema` for IDs (string with their prefix pattern), `Money`, `Currency`, `CountryCode`, `Email`, `Scope` and `Timestamp`-as-RFC-3339. Only the host turns it on, so domain crates and wasm builds don't change |
| S9 | **Errors are one schema** (`{code, error, details}`). Each operation lists its statuses; the error codes it can return go in the description for now. Generating them from each command's error type comes with MCP generation |
| S10 | **Each operation states its scope** in an `x-hypha-scopes` extension (and in its description), so generated clients and agents can see what a call needs |

## 1. What others do

- **Medusa v2:** `/admin/*` (user JWT or secret key) and `/store/*` (requires the
  `x-publishable-api-key` header, scoped to sales channels; customers authenticate on
  top), and `/auth/{actor}/…`. Its OpenAPI specs are split the same way.
- **Shopify:** two different APIs: the Admin API (app tokens with admin scopes) and the
  Storefront API (public or private storefront tokens, `unauthenticated_*` scopes), both
  GraphQL, versioned by date in the URL (`/admin/api/2026-10/…`).
- **What we take:** two surfaces with different credentials and different read models
  (both), separate specs (Medusa), a version only once there is something to keep
  stable (Shopify versions everything, but it has millions of clients).

## 2. Where today's routes go

| Today | Admin | Storefront |
| --- | --- | --- |
| `GET /store` | `GET /admin/store` | `GET /storefront/store` (name only, for now) |
| `/auth/*`, `/invites/accept` | `/admin/auth/*`, `/admin/invites/accept` | (customer auth later) |
| `/me/*` | `/admin/me/*` | |
| `/products`, `/products/{id}`, `/products/{id}/status` | same, under `/admin` | `GET /storefront/products/{id}`: active only, storefront DTO |
| markets, price lists, catalogs, tax, inventory, customers | under `/admin` | |
| `/carts/*`, `/carts/{id}/checkout` | `PUT /admin/carts/{id}/customer` | everything else: create, read, lines, email, checkout |
| `/orders/*` | under `/admin` | (orders for customers later) |
| roles, users, invites, API keys | under `/admin` | |

- **Linking a cart to a customer** moves to the admin side: until customers can log
  in, a storefront that could attach any customer ID to a cart could attach someone
  else's account. Storefronts set the email instead (checkout finds or creates the
  customer, 0007 O10).
- **Rate limits** (0009) stay on the same handlers under their new paths.
- **Idempotency keys** work the same on both surfaces.

## 3. Credentials per surface (S3)

- `Caller` grows a surface: `AdminCaller` and `StorefrontCaller` (or `Caller<Admin>`,
  `Caller<Storefront>` with a marker type). Each refuses the wrong kind of credential
  before any scope check, so a publishable key never reaches an admin command even if
  someone gave the storefront scopes an admin route needs.
- **Scopes still decide** within a surface: a secret key with `products:read` can read
  admin products and nothing else.
- **A server-rendered storefront** (Next.js) uses the publishable key too; it is not a
  secret. If a storefront needs more (customer data), that comes with customer sessions,
  not by sending a secret key to storefront routes.

## 4. OpenAPI (S6–S10)

- **How:** `#[utoipa::path]` on each handler (method, path, request body, responses,
  scope), `#[derive(ToSchema)]` on each request and response DTO, and one
  `#[derive(OpenApi)]` per surface listing its paths. Security schemes: `bearer` (session
  or secret key) for admin; `publishable_key` (`X-Publishable-Key`) for storefront.
- **Why not `utoipa-axum`'s router:** our routes are stamped out per concrete storage by
  `router!` (axum must prove handler futures `Send`), and generic handlers in its
  `routes!` macro are awkward. S7's route table gives the same guarantee (docs and
  routes can't drift) with a test instead.
- **Committed files:** `openapi/admin.json` and `openapi/storefront.json`. A test
  compares them to the generated documents and fails with "run `cargo run -p
  hypha-host-axum -- openapi` to update". The admin panel and external apps generate
  TypeScript types from these files (`openapi-typescript`).
- **OpenAPI 3.1**, `utoipa`'s default; 3.2 support is new and tools lag behind it.
- **Operation IDs** are `admin.products.create`-style, stable once released.
- **No docs UI yet:** Scalar or Swagger UI (`utoipa-scalar`) is a small addition once
  someone wants to browse; the JSON is what clients and agents need.

## 5. What changes for callers

Every path changes (S1, S4). The e2e script and the route tests move to the new paths.
Nothing is released, so there are no deprecated aliases.

## 6. Hard to reverse

- **S1 and S2:** the paths themselves.
- **S3:** which credential works where. Loosening later is easy, tightening breaks
  clients.
- **S6:** schema names and operation IDs, once clients generate from them.
- **S8:** the shape IDs and money have in the spec (strings with a prefix; money as
  `{amount, currency}` in minor units) is already the API's shape; the spec only writes
  it down.
