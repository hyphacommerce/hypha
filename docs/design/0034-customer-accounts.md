# 0034: Customer accounts: signing in with an email code

- **Status:** Proposed (2026-10-21). Danilo's answers to the five questions that shaped
  it are in §10; the build order is §11.
- **Scope:** how a shopper proves who they are on the storefront, what they can see and
  do once they have, how that reaches checkout, and what the merchant sees of it. The
  identity, session and email pieces come from 0008 and 0028; the customer record and
  its privacy rights from 0032.
- **Not in scope:** passwords and passkeys (§8 reserves them), staff logging in as a
  customer, B2B (a spore of its own, §8), customer-only prices or hidden products
  (everything stays public; accounts are optional), social sign-in.
- **Related:** 0008 §9 (`Principal::Customer`, email codes first), 0032 §9 (the customer
  record carries no login state; claiming orders is automatic; the shopper's own export
  and erasure request), 0005 C2 (one customer per email), 0023/0024 (the storefront, its
  `/account` placeholder), 0028 (templates), 0009 (rate limits), 0033 §5 (custom fields'
  visibility per surface).

## Why

Guest checkout works, and stays. What a returning shopper lacks is a way back to their
own orders, addresses and consent without an email link per order, and what a merchant
lacks is a customer who can serve themselves: reorder, change an address, unsubscribe,
download their data, ask to be erased. Those last two are rights the merchant must
answer anyway (0032 §7); an account lets the person exercise them.

The cheapest honest account is an email code (Shopify's new customer accounts, 2023).
A code proves the address, and the address is already the customer's identity (0005
C2): the moment it's proven, every guest order under it is theirs. No password to set,
forget, reset or leak; no registration form; nothing to migrate when passkeys come.

## Decisions

| | Decision |
| --- | --- |
| AC1 | **Signing in is an emailed code, and nothing else in v1** (Q1). Six digits, ten minutes, single use, five tries. Passwords are never added for customers; passkeys come later as a second way in (0008 §9) |
| AC2 | **Signing in is signing up** (Q2). A code for an email with no customer creates the customer when the code is right. No separate registration, no account state on the customer (0032 §9) |
| AC3 | **A customer session is its own token kind**, `hypha_cs_…`, sent beside the publishable key. It's a `Session` with `Principal::Customer`, and the request acts as `Actor::Customer(CustomerId)`. Hard to reverse (§9) |
| AC4 | **Authorization by ownership, not scopes.** Account routes need the customer actor, and their commands act on `ctx`'s own customer only; a customer has no scopes. The storefront surface still never carries `customers:*` |
| AC5 | **30 days, refreshed on use**, as `customers.session_lifetime` in `hypha.config.toml` (Q4); the code's life is `customers.login_code_lifetime`. Logging out everywhere revokes every session of the customer |
| AC6 | **Claiming is automatic.** One customer per email means the account holder is the customer who owns the guest orders placed with that email; nothing is transferred (0032 §9) |
| AC7 | **The shopper's privacy rights are self-service** (Q3): their export, and an erasure request with the same window and cancel as the admin's (0032 §7.3), confirmed by email |
| AC8 | **Checkout links the cart to the signed-in customer** and prefills email, default address and the consent box. A guest checks out exactly as today |
| AC9 | **Accounts are optional and everything stays public** (Q5). No store setting hides anything behind login. B2B (companies, buyers, terms) is a spore that hooks into customers, groups and this note's sessions |
| AC10 | **An erased customer has no account.** Its placeholder email can't receive a code, and its sessions are revoked by the erasure |

## 1. Model

**Identity** (`hypha-auth`, 0008):

- `Principal::Customer(CustomerId)` beside `Principal::Staff(UserId)`. One identity per
  customer, created at the first successful code; logins stay unique per principal
  kind, so a staff member and a customer may share an email.
- `Credential::EmailCode { email, hash, expires_at, attempts }`: the one credential
  kind customers have in v1. The code is six digits from the host's randomness
  (`HostRandom`), stored as a hash like every token, and replaced whole when a new one
  is asked for (one live code per customer). `attempts` counts wrong codes; the fifth
  wrong one voids the code (`code_expired` either way, so a guesser learns nothing).
- `TokenKind::CustomerSession`, prefix `hypha_cs_`: the same `Session` row as staff's
  (`sessions` gains nothing: its `principal` already names the kind), `state: active`
  at once (there's no second factor), `expires_at = now + session_lifetime`, refreshed
  on use the way staff sessions record use (`Session::record_use`, extended rather than
  only touched: a `Session::extend(lifetime)` on use when more than a day of the old
  life has passed).

**Core:** `Actor::Customer(CustomerId)`. `Ctx::customer()` answers it; `ctx.is_staff()`
stays false. Events written during an account request are stamped with it (0016).

**Customer** (`hypha-customers`): unchanged. Whether an account exists is a question for
`hypha-auth` (an identity with that principal), read by the admin's customer page, not a
flag on the customer (0032 §9).

**Cart:** `Cart::set_customer` already links a cart to a customer and takes their
email (0005 C5). Nothing new.

## 2. Flows

All on the storefront surface, with the publishable key, rate limited like the staff
login (0009): per client address, per email and address, and per email from anywhere.

**Asking for a code** `POST /storefront/auth/code {email}` → `202 {sent: true}` always,
whatever the email, so the route tells nobody who is a customer. Behind it:
`request_login_code` (`hypha-customer-accounts`, a composing crate over `hypha-auth` and
`hypha-customers`): normalise the email; refuse a placeholder (`Email::is_placeholder`,
silently: `sent: true`, nothing sent); find or create the identity for the customer
with that email, creating the customer too when there is none (AC2; a customer created
here records `customer.created` like a guest's); write the code credential; send the
`customer_login_code` email at once (`send_now`, like invites: the code is a secret and
isn't kept in a record). While the latest code is younger than
`login_code_resend_after` (a minute), asking again sends nothing and still answers
`sent: true`.

**Signing in** `POST /storefront/auth/login {email, code}` → `200 {token, customer}` or
`401 invalid_code` (wrong, expired, used up: one answer). `login_with_code` checks the
credential, voids it, creates the active session with the request's `SessionClient`
(address, user agent), and answers the token once. When the request carries a cart
(`X-Hypha-Cart`, as the storefront already sends for its cart), the cart is linked to the
customer in the same batch (AC8).

**Being signed in.** `Caller<Storefront>` learns `Authorization: Bearer hypha_cs_…` next
to `X-Publishable-Key`: the publishable key still decides the channel and the surface,
the session adds the actor. An unknown, expired or revoked session is `401
invalid_token`, as for staff. `authenticate_customer` is one read (the session and its
customer) and the occasional use-recording write.

**Signing out** `POST /storefront/account/logout` revokes the session; `POST
/storefront/account/logout-all` revokes every session of the customer (`revoke_sessions`
exists for staff).

## 3. The account surface

Every route needs `Actor::Customer`; `403 account_required` otherwise. Each command
takes the customer from `ctx`, never from input (AC4).

| Route | |
| --- | --- |
| `GET /storefront/account` | The customer: email, names, phone, addresses with defaults, consent per channel, `erasure_due_at`, custom fields the storefront may show (§5) |
| `PATCH /storefront/account` | Names and phone (0032's `update_customer` rules; the email can't be changed here in v1: it's the identity, and changing it means proving the new one, §8) |
| `POST`/`PUT`/`DELETE /storefront/account/addresses[/{id}]` | The address book, as the admin's routes |
| `GET /storefront/account/orders`, `GET …/orders/{id}` | Their orders (`OrderFilter.customer`), the order page without a token: the session is the proof |
| `PUT /storefront/account/marketing/{channel}` | Consent from the person themselves, source `storefront`; the newest consent wins as everywhere |
| `GET /storefront/account/export` | The same document as the admin's export, for the person it's about (`customer.data_requested` recorded) |
| `POST`/`DELETE /storefront/account/erasure` | The request and its cancel (0032 §7.3), with the admin's window; `customer.erasure_requested` and a confirmation email saying when it runs and how to cancel |
| `POST …/logout`, `POST …/logout-all` | §2 |

What a signed-in shopper sees of a product, a price or a category is what any shopper
sees (AC9).

## 4. Checkout and the cart

- A signed-in shopper's cart is linked at login (§2) and when a cart is created while
  signed in (`POST /storefront/carts` with a session: `set_customer` before the first
  line). The cart's email is the customer's; `PUT …/email` on a linked cart is refused
  (`email_required` already stops clearing it; changing it is refused too, since the
  order is for the customer).
- The checkout form prefills the default shipping address from the address book and
  shows the consent box as the customer's current state; ticking it writes consent
  from `checkout` as today, unticked changes nothing.
- The order confirmation and the emails are unchanged; `order.placed` carries the
  customer as it does.

## 5. Custom fields for the account holder

0033 gives customer fields `access.storefront: none | read` and never shows customers'
fields on the storefront, since it doesn't know the shopper. With a session it does:
**a customer field with `storefront: read` is shown to the account holder, for their own
values only** (`GET /storefront/account`'s `custom_fields`). No new knob; a merchant who
wants a field hidden from the person leaves it `none`. Personal fields follow the same
rule: the person may always see what's about them. Writing stays admin-only in v1.

## 6. Email

A new transactional template, `customer_login_code`, through `hypha-mail` like the
others: the merchant may edit it, the default says the code, its life and that it was
asked for from the store. Its context: `code`, `expires_in_minutes`, `store`. A second,
`customer_erasure_requested`, confirms an erasure request from the account with the due
date and the cancel route. Both are sent at once (`send_now`), not queued: the code's
value is a secret, and the request's confirmation should arrive while the person is
there.

## 7. The storefront and the admin

**Storefront** (`apps/storefront`): `/account` becomes the sign-in page (email, then the
code; the email's link `/account/login?code=…&email=…` fills it in), then the account
pages: overview with recent orders, orders, addresses, profile, marketing, privacy
(download, erase). The session token lives in an httpOnly cookie on the storefront's
server, like the cart's ID; pages under `/account` render per request. The header's
account icon goes to sign-in or the account.

**Admin:** the customer page shows "Has an account since …" and "Last signed in", a
"Sign out everywhere" button (`POST /admin/customers/{id}/sessions/revoke`), and the
customers list a `has_account` filter. Erasure (0032) revokes the customer's sessions
in its batch (AC10).

## 8. Later

- **Passkeys** as a second way in (0008 §9): the same identity, a second credential
  kind; the code remains the fallback.
- **Changing the email** from the account: a code to the new address, then the switch
  (consent resets per 0032 Q7).
- **Writable custom fields** from the account (`access.storefront: read_write` on
  customer fields: a size, a preference).
- **B2B:** a spore adding companies, buyers and payment terms over these sessions and
  0032's groups; nothing here assumes one customer per person is the end of it.
- **Social and SSO sign-in**, if ever: credential kinds like staff's SSO (0008 §9).

## 9. Hard to reverse

- **`Actor::Customer` in core and `Principal::Customer` in auth**: every host, the
  redaction rules and the event stamps branch on them.
- **The token prefix `hypha_cs_`** (0008 A9).
- **The route shapes under `/storefront/auth` and `/storefront/account`**, and that
  signing in creates the customer (AC2): a storefront built on them can't be told
  later that an email needs registering first.
- **The config keys** `customers.session_lifetime`, `customers.login_code_lifetime`,
  `customers.login_code_resend_after`.

## 10. Danilo's answers (2026-10-21)

- **Q1. Email codes only?** Yes. No passwords.
- **Q2. Signing in creates the customer?** Yes.
- **Q3. Self-service erasure?** A request, with the same window as the admin's.
- **Q4. Session length?** 30 days, refreshed on use, configurable in
  `hypha.config.toml`.
- **Q5. Anything behind login?** No: everything public, accounts optional. B2B is a
  separate spore or module that hooks into the existing system.

## 11. Build order

Each step a commit series with tests, routes, OpenAPI, SDK and e2e steps:

1. **Identity and session:** `Principal::Customer`, `Credential::EmailCode`,
   `TokenKind::CustomerSession`, `Actor::Customer`, the settings and config keys, the
   `hypha-customer-accounts` crate with `request_login_code`, `login_with_code`,
   `authenticate_customer`, `logout`, `logout_all`; the template; the storefront
   routes under `/storefront/auth` and `Caller<Storefront>` reading the session; rate
   limits.
2. **The account surface** (§3): the commands by ownership, the routes, redaction (none
   needed: the person sees their own data), custom fields for the account holder (§5).
3. **Checkout and the cart** (§4), with the storefront's prefills.
4. **Privacy from the account** (AC7): export, erasure request and cancel, the
   confirmation email; erasure revokes sessions.
5. **The storefront pages** (§7) and the admin's account facts and sign-out-everywhere.
