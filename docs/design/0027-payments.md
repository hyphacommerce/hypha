# 0027: Payments: the provider port, cash on delivery and bank transfer

- **Status:** Accepted (2026-10-18). Decided: PA2 (payment records), PA7 (the full
  set of statuses), Q1 (markets and channels), Q2 (no checkout without a method unless
  the total is zero); the rest as recommended. Danilo set the frame: a payments port with cash on
  delivery and bank transfer built in, as the seam payment spores (Stripe first) plug
  into later.
- **Scope:** payment methods a store offers (configured as data), the payments an order
  has, the order's `financial_status` derived from them, the `PaymentProvider` port and
  its built-in `manual` provider, choosing a method at checkout, recording money received
  by hand, voiding on cancel; events, routes, admin and storefront screens.
- **Not in scope:** card payments and any provider that calls out (the Stripe spore,
  after the spore API note), refunds (they need a provider that can refund; manual ones
  are recorded outside Hypha as today, 0018 Q3), partial payments by the shopper, payment
  fees and surcharges (a COD fee), gift cards and store credit, emails with the payment
  instructions (the notifications note).
- **Related:** 0002 §6 (the `PaymentProvider` sketch and the Stripe flow), 0007 (O3: two
  independent statuses; `mark_order_paid`), 0018 (Q3: cancelling a paid order leaves it
  paid), 0021 (SU8: no payment step in version 1), 0023 §4 (the storefront's payment
  step), 0016 (events).

## Why

Checkout places every order unpaid and staff mark it paid by hand (`POST
/admin/orders/{id}/paid`). That's how cash on delivery and bank transfer already work in
practice, but the shop can't say which one the shopper chose, can't show bank details,
and nothing in Hypha has the shape a card provider needs. Building the port now, with
the two manual methods as its first provider, gives every store a real payment step and
leaves Stripe as "implement the port, add a webhook route", not a redesign of checkout.

## Decisions

| | Decision |
| --- | --- |
| PA1 | **A new leaf crate `hypha-payments`** holds payment methods, payments, the provider port and its errors. Checkout (workflow) composes it; orders don't depend on it. *Hard to reverse (crate edges)* |
| PA2 | **A `Payment` aggregate** (`pay_…`), one per attempt to collect an order's money: order, method, amount and currency, status, the provider's reference. Not fields on the order: a card payment can fail and be retried, a later order edit may need a second payment, and refunds attach to a payment. *Hard to reverse (storage, API)* |
| PA3 | **The order's `financial_status` is derived from its payments and stored**, as `fulfillment_status` is from fulfilments (0018 FU3). Only payments move it; `POST /admin/orders/{id}/paid` stays, as "record the pending manual payment as received" |
| PA4 | **Payment methods are data** (`pm_…`): a provider key (`manual` built in; `spore_stripe` later), a name shoppers see, instructions (bank details), the markets and sales channels it's offered in (Q1, *Decided*), enabled or not. A store can have several manual ones (cash on delivery, bank transfer, pay at pickup). *Hard to reverse (API)* |
| PA5 | **The port is one trait, `PaymentProvider`, keyed by provider name**, listed by the host like event handlers (0019 §5.1): a tuple, static dispatch. `manual` is core's; a spore adds its own. Checkout asks the method's provider to start a payment and gets back what the shopper must do next |
| PA6 | **A cart chooses a payment method** (`PUT /storefront/carts/{id}/payment-method`), checked at checkout like shipping: offered in the cart's market and channel and enabled. Checkout writes the order and its payment in **one batch**. Without a chosen method checkout is refused (`payment_method_required`) unless the total is zero (Q2) |
| PA7 | **The full set of statuses now**, so the contract is fixed before clients depend on it: the order's `financial_status` is `pending`, `authorized`, `partially_paid`, `paid`, `partially_refunded`, `refunded` or `voided`; a payment's status is `pending`, `authorized`, `paid`, `failed`, `voided`, `partially_refunded` or `refunded`. Manual payments use `pending`, `paid` and `voided`; the rest wait for providers that move money that way. *Decided; hard to reverse (contract)* |
| PA8 | **Cancelling an order voids its pending payments** (COD never collected, transfer never sent); a paid one stays paid, as 0018 Q3 decided |
| PA9 | **New scopes `payments:read` and `payments:write`**: methods are settings (`payments:write`); recording money received stays `orders:write`, like today's `/paid`, since it's what staff do on an order |

## 1. What others do

- **Shopify:** "manual payment methods" (cash on delivery, bank deposit, money order,
  custom) with instructions shown at checkout and on the confirmation; the order is
  created with payment *pending* and staff "mark as paid" when the money arrives. Every
  money movement is a *transaction* on the order (authorization, capture, sale, void,
  refund); the order's financial status is derived from them.
- **Medusa v2:** a Payment module with *payment collections* (what an order owes),
  *payment sessions* (one per provider attempt) and *payments*; providers implement one
  interface (`initiatePayment`, `authorizePayment`, `capturePayment`, `refundPayment`,
  `getWebhookActionAndData`). A built-in `pp_system_default` provider does nothing and
  is what "manual" payments use. Regions choose which providers are offered.
- **Saleor:** payment *apps* through webhooks (transactions API); manual payments are
  "mark as paid" with a transaction reference.

Hypha takes Shopify's model of manual methods with instructions, Medusa's single
provider interface with a do-nothing built-in, and Medusa's per-region availability
(markets here).

## 2. Model (`hypha-payments`)

**`PaymentMethod`** (`pm_…`, aggregate root, `Versioned`, `Stamped`):
- `provider: ProviderKey`: `manual`, or a spore's (`spore_stripe`), checked against the
  providers the host lists when created;
- `name` (shown to shoppers, "Cash on delivery"), `instructions` (plain text, shown after
  checkout and on the order: bank details, "pay the courier");
- `markets: Vec<MarketId>` and `channels: Vec<ChannelId>` (empty = every one), `enabled:
  bool`, `position` (the order shoppers see them in);
- `settings: serde_json::Value`, the provider's own settings, checked by the provider
  (none for `manual`; a spore's keys live in its secrets, not here).

**`Payment`** (`pay_…`, aggregate root):
- `order: OrderId`, `method: PaymentMethodId`, `provider: ProviderKey` (copied, so a
  deleted method can't orphan it);
- `amount: Money` in the order's currency, the whole total for now;
- `status: PaymentStatus` (PA7): `pending` → `authorized` → `paid` →
  `partially_refunded` → `refunded`, or `voided` (cancelled before money moved) or
  `failed` (a provider's decline) from `pending` or `authorized`; the domain refuses any
  other move;
- `refunded: Money`, what was given back (zero until refunds exist);
- `reference: Option<String>` (the provider's ID; for manual, what staff typed: a bank
  reference, the courier's receipt);
- `instructions: Option<String>`, copied from the method at checkout, so later edits to
  the method don't change what a shopper was told;
- `paid_at`, `voided_at`.

**The order** keeps `financial_status`, now computed from its payments when one changes:
`paid` once paid amounts (less refunds) cover the total, `partially_paid` when some but
not all, `authorized` when authorizations cover it and nothing is captured yet,
`refunded` / `partially_refunded` from the refunds, `voided` when every payment was
voided, else `pending`. Orders don't store payments; the batch that changes a
payment updates the order in the same commit, so the two never disagree.

## 3. The port

```rust
#[allow(async_fn_in_trait)]
pub trait PaymentProvider {
    /// "manual", "spore_stripe".
    fn key(&self) -> ProviderKey;
    /// Checks a method's settings when staff save it.
    fn check_settings(&self, settings: &serde_json::Value) -> Result<(), PaymentError>;
    /// Starts collecting `payment` (already staged, not yet committed): what the
    /// shopper must do next.
    async fn start(&self, payment: &PaymentRequest) -> Result<NextAction, PaymentError>;
}

pub enum NextAction {
    /// Nothing online: show the instructions (cash on delivery, bank transfer).
    Instructions(String),
    /// A provider's page or widget: card fields, a redirect.
    ClientSecret { secret: String },
    Redirect { url: String },
}
```

Capture, void and refund calls, and parsing a provider's webhooks, are added with the
first provider that needs them (Stripe); `manual` needs none, since staff record what
happened. The trait has no `Send` bound (Workers), like every port. The host lists its
providers in `payments.rs` next to `handlers.rs`; checkout and the method commands find
one by key.

## 4. Checkout

1. The storefront lists the cart's methods: `GET /storefront/carts/{id}/payment-methods`
   (enabled, offered in the cart's market, in position order: name and provider).
2. The shopper chooses one: `PUT /storefront/carts/{id}/payment-method` stores
   `payment_method` on the cart (one more field).
3. `POST /storefront/carts/{id}/checkout` checks the method again (`payment_method_required`
   when the store has enabled methods and none is chosen, `payment_method_unavailable`
   when it's no longer offered), then in the same batch as the order: a `Payment`
   (`pending`, the order's total, instructions copied). After the commit it calls the
   provider's `start` and answers with the order **and the next action**.
4. For `manual`, the next action is the instructions; the confirmation page shows them,
   with the order number as the transfer reference.

A provider that fails to start (Stripe down) leaves the order placed and the payment
`pending`; the storefront can ask again (`POST /storefront/orders/{id}/payment`, with
the order's access token), which a later provider note details. For `manual` it can't
fail.

## 5. Recording and voiding

- `POST /admin/orders/{id}/paid` (exists): with an optional `reference`, records the
  order's pending payment as `paid` and the order as `paid`, in one batch. An order with
  no payment (placed before this, or with no method enabled) gets a `manual` payment
  created and paid on the spot, so the route keeps working for every order.
- `POST /admin/orders/{id}/cancel` (exists): also voids pending payments (PA8).
- Idempotent as today: marking a paid order again changes nothing.

## 6. Events

- `payment.created`, `payment.paid`, `payment.voided` (subject: the payment; data: order,
  method, provider, amount).
- `order.paid` stays, recorded when the order's status becomes `paid`; webhooks that
  listen to it keep working.

## 7. API and screens

- Admin: `GET/POST /admin/payment-methods`, `GET/PATCH/DELETE
  /admin/payment-methods/{id}` (a method in use by a pending payment can be disabled, not
  deleted); the order response gains `payments`.
- Storefront: §4's two cart routes; the order and checkout responses gain `payment`
  (status, method name, instructions).
- Admin screens: **Settings → Payments** (add cash on delivery or bank transfer from a
  preset, edit name, instructions, markets; enable, reorder); the order page's payment
  card shows the method, status and instructions, with **Mark as paid** (reference
  field). The setup guide (0021) gains "choose how customers pay".
- Storefront: a payment step in checkout (radio list of methods with their
  instructions), the confirmation page shows the instructions.
- `scripts/seed.mjs` adds cash on delivery and bank transfer; `e2e.sh` gains steps.

## 8. Build order

1. `hypha-payments`: methods, payments, statuses, the port and `ManualProvider`;
   repository traits and the contract (in-memory, then Postgres, migration).
2. Orders: `financial_status` gains the full set (PA7), set from payments; `order-actions`'s
   cancel voids.
3. Checkout: cart's `payment_method`, the checks, the payment in the batch, the next
   action.
4. Routes, OpenAPI, SDK; e2e steps.
5. Admin: Settings → Payments, the order's payment card.
6. Storefront: the payment step and confirmation instructions; the seed.

## 9. Open decisions

**Q1. Methods per market or per channel?** *Decided:* both. A method lists the markets
and the sales channels it's offered in (empty = all), so cash on delivery can be for
Serbia only and kept off the agentic channel.

**Q2. Must a store have a method?** Shopify and Medusa v2 both refuse checkout without
one (Shopify: the store "can't accept payments right now"; Medusa: no payment session),
except for orders that total zero, which need no payment. Both make a manual method the
easy default (Medusa's `pp_system_default`). PA6 proposed the opposite (checkout as
today without a method). *Decided:* follow them: `payment_method_required`
unless the total is zero; the setup guide and the seed add cash on delivery and bank
transfer, and the admin warns while no method is enabled.

**Q3. Partial payments by hand?** Recommended: not now: "mark as paid" covers the whole
total. A deposit flow (pay 30% now) needs `partially_paid` and amounts per payment,
which the model allows later without a migration of meaning.

**Q4. A COD fee?** Recommended: later, as a cart adjustment, not a payment field: it
changes the order total, tax and the shopper's price display, so it belongs with
pricing.
