# 0009: Rate limiting

- **Status:** Accepted and built (2026-10-14, branch `feat/rate-limiting`). Danilo
  accepted the defaults below in conversation ("write 0009 with those defaults"); the
  numbers in section 3 are starting points, tuned once there is traffic to look at.
- **Paths:** since 0010, the routes here live under `/admin` (`/admin/auth/login`, …);
  the paths below are as first built.
- **Scope:** limits on the routes that need no credentials (login, the second factor,
  password reset, invite acceptance), the client IP behind proxies, and the port that
  lets each host keep the counts where it can.
- **Not in scope:** limits per API key or per staff member on authenticated routes,
  limits for the storefront, and caching (section 6 says why there is no cache yet).
- **Related:** 0008 §9 (rate limiting was its first follow-up, and closes its two known
  limits), 0006 (one store per deployment), 0001 (hosts).

## Decisions

| | Decision |
| --- | --- |
| R1 | **Use [`governor`](https://crates.io/crates/governor), don't write a limiter.** GCRA (a smooth token bucket), keyed, in memory. Not `tower_governor`: as middleware it runs before the body is read, so it can't limit per email, and we need different keys per route |
| R2 | **A `RateLimiter` port in the host.** `check(rule, key)` answers allowed, or limited with a retry delay. The in-memory `governor` version is the only one for now; a shared one (Postgres, or the Workers binding) comes with the first host that runs more than one instance |
| R3 | **Limit per address, per email and address, and loosely per email** on the public routes (section 3). Per address stops one client hammering; per email and address stops guessing at one account from one place; the loose email limit bounds guessing spread over many addresses without letting one stranger lock the owner out. The second-factor lockout (0008) already limits per account |
| R4 | **The client IP comes from the socket**, unless the request came through a proxy listed in `HYPHA_TRUSTED_PROXIES` (CIDR ranges): then from `X-Forwarded-For`, read right to left up to the first address that isn't a trusted proxy. With the variable unset, `X-Forwarded-For` is ignored, so a client can't pick its own key |
| R5 | **IPv6 addresses are keyed by their /64.** One customer usually gets a whole /64, so keying by full address would let them rotate through billions of keys |
| R6 | **Over the limit is `429 rate_limited`** with `Retry-After` (seconds) and `details: {"retry_after_secs": n}`, checked before any password hashing, storage or email work |
| R7 | **No cache or KV store yet.** Nothing built so far needs one (section 6); a cache port comes when there is something measured to speed up |

## 1. What others do

- **Shopify:** its Admin API limits each app per store with a leaky bucket; the limits
  on its own login pages aren't documented.
- **Cloudflare Workers:** a [rate limiting binding](https://developers.cloudflare.com/changelog/post/2025-09-19-ratelimit-workers-ga/index.md)
  (stable since September 2025), keyed by whatever the Worker passes, counted across
  Cloudflare's network. In `workers-rs` it is `RateLimiter`.
- **OWASP:** limit by IP and by account; mind that limits by account let an attacker lock
  a real user out, so keep them loose enough that a user rarely hits one.

## 2. The port (R2)

```rust
// hypha-host-axum
pub enum Rule {
    LoginIp, LoginEmailFromIp, LoginEmail, MfaIp,
    ResetIp, ResetEmailFromIp, ResetConfirmIp, InviteIp,
}

pub enum Decision { Allowed, Limited { retry_after: Duration } }

pub trait RateLimiter {
    async fn check(&self, rule: Rule, key: &str) -> Decision;
}
```

- **In the host**, not in core or a domain crate: rate limiting is about requests, like
  authentication (CLAUDE.md: authentication lives in hosts). When a second host exists
  (Workers, Lambda), the trait and the rules move to a small crate both hosts share.
- **Async**, because a shared version waits on the network.
- **`Rule` is a closed list** with its quota next to it, so a handler can't invent a
  limit and every limit is in one place.
- **The in-memory version** keeps one keyed `governor` limiter per rule. Keyed state
  grows with every new key, so a background task drops idle keys every minute
  (`retain_recent`); otherwise rotating IPs would grow memory without bound.
- **Tests** use `governor`'s fake clock, so a test steps time instead of sleeping.
- **A shared version later:**
  - Postgres: a `rate_limits (key, window_start, count)` table, an upsert per check,
    a fixed window. Good enough at login-route volumes, no new service.
  - Workers: the platform binding.
  - Redis or Valkey only if a deployment outgrows Postgres for this.

## 3. Rules and starting numbers (R3)

GCRA describes a quota as a burst plus a steady rate.

| Rule | Route | Key | Burst | Then |
| --- | --- | --- | --- | --- |
| `LoginIp` | `POST /auth/login` | address | 10 | 1 every 30 s |
| `LoginEmailFromIp` | `POST /auth/login` | email and address | 5 | 1 every 60 s |
| `LoginEmail` | `POST /auth/login` | email | 50 | 1 every 72 s (about 50 an hour) |
| `MfaIp` | `POST /auth/login/mfa` | address | 10 | 1 every 30 s |
| `ResetIp` | `POST /auth/password-reset` | address | 5 | 1 every 60 s |
| `ResetEmailFromIp` | `POST /auth/password-reset` | email and address | 3 | 1 every 5 min |
| `ResetConfirmIp` | `POST /auth/password-reset/confirm` | address | 10 | 1 every 30 s |
| `InviteIp` | `POST /invites/accept` | address | 10 | 1 every 30 s |

- **Order in a handler:** the address rule first, before the body is parsed; then the
  body; then the email rules; then the work (hashing, storage, email). A limited
  request costs almost nothing.
- **Every attempt counts**, right or wrong. Counting only failures would need the
  answer first, and the answer costs a password hash.
- **Emails as keys** go through `Email::new`: the same normalisation as a login, so one
  account is one key, and at most 254 bytes, so a key can't be megabytes of body.
  Something that isn't an email gets no email limit (it can't match an account), only
  the address one.
- **No lockout by email alone** (*changed after the review, 2026-10-14*): a strict limit
  keyed by the email alone let one stranger, sending a request each time it refilled,
  keep the real owner locked out indefinitely. The strict email limit is now per email
  *and* address, which bounds guessing from one place; the email-only limit is loose
  (about 50 an hour), which bounds guessing spread over many addresses while locking
  someone out takes a great many of them. Password reset has no email-only limit at
  all: the domain already sends at most one email per account every 5 minutes, and a
  stranger asking only sends the owner a valid link.
- **Confirming a reset** has its own bucket, so asking for a few links doesn't block
  using one.

## 4. The client IP (R4, R5)

- **Default:** the socket's peer address. Right for a VPS reached directly.
- **Behind a proxy** (a load balancer, Fly.io, Render, Cloudflare in front of a VPS):
  every request would come from the proxy's address, so everyone would share one key.
  Set `HYPHA_TRUSTED_PROXIES` to the proxy's ranges, e.g.
  `HYPHA_TRUSTED_PROXIES=10.0.0.0/8,fd00::/8`. A request from a trusted peer is keyed by
  the rightmost `X-Forwarded-For` address that isn't itself trusted; one that isn't
  from a trusted peer is keyed by its peer address, whatever the header says.
- **Never trust `X-Forwarded-For` by default:** anyone can send one, and a limit keyed
  by it would be a limit the attacker picks.
- **A dedicated header** (`CF-Connecting-IP`, `Fly-Client-IP`) is a possible later
  addition; `X-Forwarded-For` with trusted ranges covers all of them for now.
- **IPv6 /64:** see R5. IPv4 addresses are keyed whole.

## 5. What this closes from 0008

- **Reset requests crowded out:** a flood from a few addresses now meets `ResetIp`
  before it reaches the background slots. A flood spread over very many addresses can
  still fill them; that needs the network's help (a CDN or WAF), not more code here.
- **Work before the token check:** authenticated routes are unchanged (their limits
  come with per-key limits later). The public routes now refuse a flood before parsing
  or touching storage.
- **Password guessing:** `LoginEmailFromIp` bounds guesses at one account from one
  address, `LoginEmail` (looser) however many addresses are used, and `LoginIp` one
  address trying many accounts.

## 6. Why no cache or KV store yet (R7)

- **Rate limits:** one instance needs none (memory); several instances need shared
  counts, and Postgres or the Workers binding do that (section 2).
- **Idempotency keys** must stay in Postgres: they commit in the same transaction as
  the order, which is what makes a replay safe.
- **Sessions** are read on every request (session, user, role). Caching them would
  bring back the stale permissions server-side sessions were chosen to avoid (0008 A3).
- **Catalog and pricing reads** are the real candidate: read often, changed rarely.
  That is where [Medusa's Caching Module](https://docs.medusajs.com/resources/infrastructure-modules/caching)
  (v2.11, Redis, in preview) earns its [published gains](https://medusajs.com/blog/medusa-cache-benchmarks/),
  and what [Medusa Cloud](https://docs.medusajs.com/cloud/cache) sells. Hypha has no
  listing endpoints yet and no measurements. When it does, the cheapest large win is
  HTTP caching first (`Cache-Control` and `ETag` on public storefront reads, served by
  a CDN), and a `Cache` port with Redis, Workers KV or memory behind it second.
- **OAuth state** (SSO later) belongs in Postgres or a port with a durable version:
  Medusa keeps it in its cache module and falls back to memory without Redis, which
  breaks logins across instances.

## 7. Known limits

- **IPv6 ranges wider than /64** (third review, 2026-10-14): someone holding a /48 gets
  65,536 separate address keys. On the routes that only have an address limit (the
  second factor, reset confirmation, invite acceptance) that limit is about load, not
  guessing: getting through needs a 256-bit token or a valid challenge, and the
  second-factor lockout counts per account (0008). Login is bounded by the email limits.
  A coarser /48 bucket can follow if traffic shows such rotation.
- **One instance's counts** until a shared `RateLimiter` exists (section 2).

## 8. Later

- Limits per API key and per staff member on authenticated routes (Shopify's bucket
  per app), with `RateLimit-*` headers.
- A shared `RateLimiter` (Postgres; the Workers binding) with the first multi-instance
  host.
- Quotas from configuration rather than constants, once there is traffic to tune by.
- A dedicated client-IP header option.

## 9. Hard to reverse

Little: the port, the rules and the 429 shape are internal or additive. The one
operational trap is R4: a deployment behind a proxy that doesn't set
`HYPHA_TRUSTED_PROXIES` limits everyone together, and the startup log should say which
mode it runs in.
