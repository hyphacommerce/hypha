# 0031: Payment-first checkout

- **Status:** Accepted (2026-10-20). Danilo chose payment first (B) over paying on the
  order page: an order paid online exists only once its money is committed. Decided:
  Q1 (the payment is started for the cart and attached to the order, Shopify's way),
  Q2 (the order-first contract is replaced outright: pre-1.0, test stores only),
  Q3 (`processing` counts as committed).
- **Scope:** when a payment is started (on the cart, before the order), when checkout
  completes (once the provider says the money is committed, from the shopper's browser
  or the provider's webhook), the `processing` status, the provider port and the
  `payments.*` points, the storefront's checkout and confirmation.
- **Not in scope:** express checkout (wallets from the cart or product page: this makes
  it possible, a later note), split payments, an "abandoned checkouts" admin view,
  expiring abandoned cart payments (see §7).
- **Related:** 0027 (payments, the port, `financial_status`), 0029 §3.1 and §7 (the
  `payments.*` points, storefront elements), 0030 (the Stripe spore), 0015 (shipping
  quotes checkout renews).

## Why

Until now checkout placed the order first, with a pending payment, and the shopper paid
on the order confirmation. A shopper who gave up left an unpaid order holding stock,
and the card form appeared one page after the shopper pressed "Place order". Medusa v2,
Shopify, Saleor and commercetools collect the money against the cart and create the
order once it's authorized; WooCommerce and Magento are order-first. Payment first is
what merchants expect ("an order means the money is there") and what wallets and
express checkout need.

## Decisions

- **Q1. Where the payment lives before the order.** A `Payment` belongs to its cart
  (`cart`) from the start and to its order (`order`) once checkout completes; one
  aggregate, so the attempt check, give-backs, capture, void and refund work unchanged.
  Money staff record by hand on an old order has an order and no cart.
- **Q2. The old contract.** Replaced, not kept beside: `payments.start` is version 2
  (a cart, no order), `POST /storefront/orders/{id}/payment` is gone, and so is the
  confirmation page's payment widget. Apps on version 1 (the separate Stripe app) stop
  working until they move.
- **Q3. Slow methods.** A provider may report `processing` (SEPA Direct Debit, some bank
  methods): the money is committed but not collected. Checkout completes on it; the order
  is placed with the payment `processing` (its `financial_status` `pending`), so staff
  see it isn't paid yet and don't ship.

## 1. The flow

1. The shopper fills in the details and shipping and chooses a method.
2. **Manual methods** (cash on delivery, bank transfer) and a zero total: "Place order"
   checks out as before: the order and its pending payment in one batch, the method's
   instructions on the confirmation.
3. **Online methods:** the storefront starts the cart's payment
   (`POST /storefront/carts/{id}/payment`, with the total the shopper sees). Hypha runs
   checkout's checks (lines, email, shipping, total, method), makes a pending payment of
   the cart's total with no order, and asks the provider to start: the card form
   (`client_widget`) appears on the checkout step, or the shopper goes to the provider's
   page (`redirect`).
4. The shopper pays in the widget. On `hypha:payment-confirmed` (or back from the
   provider's page) the storefront checks out the cart. Checkout finds the cart's
   payment, and if it's still pending asks the provider where it stands
   (`PaymentProvider::status`), records that, and completes once the payment is
   committed: `authorized`, `paid` or `processing`.
5. If the shopper never comes back, the provider's webhook reports the payment committed
   and the host completes the checkout itself (`complete_paid_checkout`). Both ways run
   the same command; the cart becomes an order once (`ORDER_CART`), and the second
   caller is answered with that order.

## 2. Model (`hypha-payments`)

- `Payment::cart: Option<CartId>`, `Payment::order: Option<OrderId>` (at least one);
  `Payment::attach(order)` once, when checkout completes, staged in its batch.
- `PaymentStatus::Processing`: `pending → processing → paid | failed`, and
  `pending → processing` only from `pending`. Not voidable (the money's on its way).
  `financial_status` counts it as `pending`. "Committed" is `authorized`, `processing`
  or captured (`PaymentStatus::committed`).
- `PaymentRepo::list_for_cart` (newest first). Events carry `cart` and `order` (`null`
  before the order).

## 3. Checkout (`hypha-checkout`)

- `start_cart_payment`: the checks, then the cart's newest payment: one pending of the
  same method and amount is started again (the provider resumes its attempt); other
  pending ones are voided (and the provider asked to release them, best effort) and a
  new one made; a committed one isn't started again (`cart_payment_committed`: check
  out instead). Refused for a manual method (`payment_method_offline`: place the order).
- `checkout`: for an online method, the cart's newest payment of that method must be
  committed (asking the provider first when it's pending): none is `payment_required`,
  still pending `payment_pending`, failed or voided `payment_failed`. Its amount must be
  the cart's total (`total_changed` otherwise). Then the order is placed as before, with
  the payment attached in the same batch and the order's `financial_status` from it.
- If checkout can't complete once the money is committed (a line sold out meanwhile, the
  total changed, the shipping option went away), the money goes back: an authorized
  payment is voided, a paid one refunded, through the provider, before the error is
  answered. A `processing` one can't be yet; it's logged for staff.
- A completed cart is answered with its order (`Placed`, replayed), not `cart_completed`:
  the browser and the webhook may both finish it.

## 4. The port and the points

- `PaymentRequest` names the cart and, for a manual method's start, the order (`None` for
  a cart payment).
- `PaymentProvider::status(&StatusRequest) -> ProviderStatus` (`open`, `processing`,
  `authorized`, `paid`, `failed`, `canceled`, with the provider's reference and attempt):
  what checkout asks a pending payment. The default is `payment_action_unsupported`
  (then checkout waits for the provider's report: `payment_pending`).
- Apps: `payments.start` version 2 (the cart, no order) and a new point
  `payments.status` (version 1).
- `record_payment_event` takes reports on a payment without an order (no order to
  update) and `processing`. The admin events route and the Stripe webhook complete
  the checkout when a report commits a cart payment.

## 5. Storefront

- The payment step, Shopify's way: once the details and shipping are in and an online
  method is chosen, the cart's payment starts and the provider's form appears under the
  method, its own button hidden (`submit_mode: "external"`, `docs/apps/elements.md`);
  the summary's one "Pay €X" button calls the element's `submit()`. On
  `hypha:payment-confirmed` the order is placed and the shopper lands on the
  confirmation; a decline shows under the form and the button, and the shopper tries
  again on the checkout (as Shopify does, no error page). A redirect method's "Pay" goes
  to the provider's page, which comes back to `/<country>/checkout/complete` (it checks
  out only a cart paying online).
- The confirmation shows the order's payment status and, for manual methods, the
  instructions; it no longer starts payments.

## 6. Stripe

- The intent is made for the cart's payment (metadata `hypha_payment`, `hypha_cart`),
  `status` reads it (`requires_capture` → authorized, `succeeded` → paid,
  `processing`, `canceled`, else open), the webhook reports
  `payment_intent.processing`, and completes the checkout after a report that commits
  a cart payment.

## 7. Left for later

- Expiring abandoned cart payments. A pending intent holds nothing; an authorized one
  whose checkout never completed (the webhook's completion keeps failing) would hold the
  card until Stripe releases it (7 days). A job voiding authorized cart payments older
  than an hour is the follow-up.
- `processing` payments whose checkout fails can't be given back automatically.
