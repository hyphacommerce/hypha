# 0038: Discounts: codes, automatic discounts, buy X get Y, tiers and free shipping

- **Status:** Accepted (2026-10-24): Danilo accepted the proposals, with discounts additive
  within a class (DS5). §14 lists what was open.
- **Scope:** cart-level reductions, as layer (b) on top of 0037's prices.
  - The `Discount` aggregate: automatic or by code; product, order and shipping classes;
    typed conditions and targets; percentage, amount, fixed-price and free values;
    standard, buy X get Y and tiered kinds.
  - How discounts combine.
  - The pure engine that applies them and allocates them to lines and shipping, to the
    cent.
  - Codes at scale, and usage limits counted inside checkout's batch.
  - What carts, orders, emails, the admin and the storefront show.
- **Not in scope:**
  - Sales, clearance, volume prices per variant and group prices: those are price lists
    (0037).
  - Gift cards and store credit: payment, sketched in §12 and given their own note.
  - Manual discounts on draft orders (no draft orders yet).
  - Subscriptions.
  - Auto-adding free gifts to the cart.
  - App- or spore-computed discounts (reserved, §11).
  - Segments (0032 §8).
- **Related:**
  - 0037 (the prices discounts start from: `ResolvedPrice`, `on_sale`).
  - 0003 (tax), 0015 (shipping rates), 0027 / 0031 (payments, payment first).
  - 0029 §11 (`discounts.calculate`, wasm functions).
  - 0032 (customer groups), 0034 (accounts).
  - 0035 (agents: every command is a typed operation).
  - The UCP note in CLAUDE.md (per-line problems as `messages` with a `path`).

## Why

Every store needs these at launch, and Hypha has none of them:

1. **Discount codes:** `WELCOME10`, influencer codes, 5,000 single-use codes for a
   newsletter.
2. **Automatic promotions:** "free shipping over €50", "10% off the order this
   weekend".
3. **Buy X get Y:** "buy a grow kit, get a culture syringe free".
4. **Volume discounts across a cart:** "any 3 cultures: 10% off, any 5: 15%";
   "spend €100, save €15".

Each one has to be right to the cent, interact with tax correctly, survive refunds
later, and never oversell a limited code under concurrent checkouts. That's where every
platform surveyed has a known flaw (§1).

## Decisions

| | Decision |
| --- | --- |
| DS1 | **A new leaf crate, `hypha-discounts`** (`disc_…`): the aggregate, its codes, usage, events and a **pure engine**. The engine takes plain values (lines, prices, categories, the buyer, shipping amounts) that the cart builds, like `resolve_price` (§2, §5) |
| DS2 | **Classes are combination categories, separate from the value**: a discount has one or more of `product`, `order`, `shipping` and `combines_with` the same three. Two discounts can apply together only if *each* allows every class of the other. Among the valid sets, **the one that saves the buyer most wins** (§4) |
| DS3 | **Typed conditions and targets, a closed enum**, not a string language: markets, channels, customer groups, customers, signed-in, minimum subtotal or quantity, and item sets (all, products, variants, categories with descendants, excluding items on sale). All conditions must hold; a condition's values match any (§3) |
| DS4 | **Fixed amounts are per market** (`{mkt_…: Money}`); percentages carry none. In a tax-inclusive market an amount is gross, as the shopper sees prices there. A discount with no amount for the cart's market doesn't apply there (§3) |
| DS5 | **Additive within a class, Shopify's model**: product discounts on lines, then order discounts on the subtotal after product discounts, then shipping. Discounts of one class are each computed on the **same base** (the class's starting amount) and added up, so 10% + 20% is 30%. The order within a class never changes a total. Nothing goes below zero (§5) |
| DS6 | **Allocation to the cent, by largest remainder**: an order discount, or an `across` amount, is split over the eligible lines in proportion to their remaining amounts. Leftover minor units go to the largest fractional parts, ties to the earlier line, so the shares sum exactly (Vendure's `prorate`) (§5) |
| DS7 | **Codes are rows of their own**: many per discount, normalised (trimmed, case-folded), unique in the store, generated in bulk. A cart holds up to 5. Each code on a cart reports a **state**, which maps onto UCP messages (§6) |
| DS8 | **Usage is counted in checkout's batch**: a conditional increment on a counter row kept apart from the discount (so it never bumps the discount's version), plus a usage row unique per (discount, customer) for once-per-customer. A lost race is a typed `discount_limit_reached`. A cancelled order doesn't give the use back (§7) |
| DS9 | **Orders record applications and allocations**: what applied on the order, how much on each line and shipping line. A line's `total` becomes the amount after discounts. Tax is on that amount (§8) |
| DS10 | **One `summary()` in the domain**, a sentence built from the discount's data ("10% off Grow kits · orders over €50 · until 31 Oct"). The admin, emails, the storefront and agents all show the same words (§10) |

## 1. What others do

The research was done on 2026-10-24 against current docs and source.

### Shopify

Sources: [discounts](https://shopify.dev/docs/apps/build/discounts),
[combinations](https://help.shopify.com/en/manual/discounts/combining-discounts/discount-combinations),
[DiscountAllocation](https://shopify.dev/docs/api/admin-graphql/latest/objects/DiscountAllocation).

- **Types.** Eight node types: basic, buy X get Y, free shipping and app, each as a code
  or automatic.
- **Classes.** Since 2025-04, `discountClasses` (product, order, shipping) are plural,
  so one discount can be "20% off X plus free shipping". The class decides combination,
  not the value.
- **Eligibility** (`context`): everyone, specific customers, segments or markets.
- **Minimums:** a subtotal or a quantity. For order discounts the subtotal is checked
  both before and after product discounts.
- **Usage:** `usageLimit` and `appliesOncePerCustomer`, counted asynchronously, so the
  count lags.
- **Codes:** many per discount, 250 per bulk call, 20M per store.
- **Values:**
  - Percentage.
  - An amount "on each item", or spread proportionally across the entitled lines.
  - Buy X get Y: buys by quantity or amount, gets a quantity at a percentage or free,
    with a per-order use limit, and the cheaper qualifying item discounted.
- **Combining.**
  - `combinesWith {product, order, shipping}`, which both discounts must allow. When
    discounts can't combine, "the best discount or combination" applies.
  - Order: product, then order (on the subtotal after product discounts), then
    shipping.
  - Several percentage order discounts are each computed on the same subtotal, not
    compounded.
- **Limits:**
  - 25 automatic discounts per store.
  - Per order, 5 codes plus 1 shipping code.
  - Several product discounts on one line need Plus.
  - Product + order combinations only on "eligible" stores.
- **On the order.** `discountApplications` (what: allocation method, target type,
  value, index) on the order and `discountAllocations` (how much) on each line and
  shipping line. This is the model to copy.
- **Functions.** Pure wasm functions that propose candidates; Shopify applies
  combinations, caps and allocation.
- **Tax** is on the discounted amount. In tax-inclusive markets the discount comes off
  the gross price.
- **Fixed amounts** are in the shop's currency, converted at checkout (a known source
  of rounding bugs).
- **Refunds.** A refunded line returns its net share of an order discount, and nothing
  is redistributed. This is the most common merchant complaint.
- **A sale** (compare-at) isn't a discount.

### Medusa v2

Source: the promotion module.

- **Promotion:** `code`, `is_automatic`, `type` standard / buyget, `status`, and
  `limit` / `used`. Rules are a flat AND of `attribute op values`.
- **ApplicationMethod:** `type` fixed / percentage, `target_type` items / shipping /
  order, `allocation` each / across / once (the cheapest units first), and
  `max_quantity`.
- **Campaigns** carry budgets (usage, spend, per attribute: "once per customer").
- **`computeActions`** is a pure function returning add/remove adjustment actions, and
  the workflow persists them.
- **Stacking:** every code stacks. Buy-get promotions come first, then by value
  descending. There's no exclusivity.
- **Weaknesses:**
  - All arithmetic is BigNumber with **no rounding**: fractional cents are stored.
  - One code per promotion.
  - `max_quantity = 0` means two different things.
  - A reported overspend race on limits (read, check, write).

### Saleor

- **Catalogue promotions** are precomputed onto prices (with lag).
- **Order promotions:** the best one wins, rewards are a subtotal discount or a gift.
- **Vouchers:** many codes each, usage limits, once per customer, once per order (the
  cheapest unit), per-channel values and minimums.
- **Stacking:** catalogue + (best order promotion **or** voucher), then tax.
- **Records:** discount rows per checkout, line, order and order line, with at most one
  of each type per line.

### Vendure

- **Rules as code:** conditions and actions are plugin-registered code with typed
  args.
- **Stacking:** everything that passes stacks, sorted by an ascending priority score.
- **Allocation:** order discounts are prorated to lines by **largest remainder**, and
  `proratedUnitPrice` is what tax and refunds use. That's the right model.
- **Weaknesses:**
  - One coupon per promotion.
  - Usage counting isn't transactional (a soft hold during payment).
  - The rules are opaque to admins and agents.

### commercetools

- **Product discounts:** one per price, precomputed.
- **Cart discounts:**
  - Predicates in a string language.
  - Targets: lines, shipping, total, multi-buy, patterns.
  - A sort order (higher first) and `StopAfterThisDiscount`.
  - Discount Groups (2025: the best of a group applies), because sort orders became
    unmanageable.
  - A project-wide `BestDeal` mode.
- **Codes:** many to many, with groups, `maxApplications` (per customer too), up to 10
  per cart.
- **A state per code on the cart:** `MatchesCart`, `DoesNotMatchCart`,
  `MaxApplicationReached`, `ApplicationStoppedByPreviousDiscount`… This is the UX
  contract to copy.
- **Line splits:** lines are split into packages (`discountedPricePerQuantity`) when
  units get different discounts.

### Solidus

Promotions with conditions and benefits, applied in **lanes** (pre / default / post):
members of a lane compete, and lanes chain.

### What we take and what we refuse

What we take:
- Shopify's classes and `combines_with`, best combination, application order, and
  applications plus allocations on the order.
- Medusa's each / across / cheapest allocations and its pure engine.
- Vendure's largest-remainder proration.
- commercetools' code states.
- Saleor's separate code table with once-per-customer rows.

What we refuse:
- String predicate languages: agents and the admin edit typed data.
- Unrounded money.
- One code per discount.
- Usage counted outside the order's transaction.
- Shopify's caps as product limits. Hypha's caps are performance constants, stated
  (§5).

## 2. The model

```rust
pub struct Discount {
    id: DiscountId,                 // disc_…
    version: Version,
    stamps: Option<Stamps>,
    title: String,                  // shown to buyers ("Welcome offer")
    method: Method,                 // Automatic | Codes
    status: Status,                 // Draft | Active (expired / scheduled are derived)
    starts_at: Option<Timestamp>,
    ends_at: Option<Timestamp>,
    classes: ClassSet,              // ⊆ {product, order, shipping}, non-empty
    combines_with: ClassSet,
    conditions: Vec<Condition>,     // all must hold (§3)
    kind: Kind,                     // Standard | BuyXGetY | Tiered (§3)
    limits: Limits,                 // total uses, once per customer, uses per order
    events: Recorder,
}
```

- **Classes follow the kind and its targets**, checked by the constructor:
  - A `Standard` targeting items is `product`, the subtotal is `order`, shipping is
    `shipping`.
  - Since Shopify's 2025 change, one discount may carry several **rewards** (§3), each
    with its own target. That's "20% off cultures + free shipping" as one discount.
- **Status.** `draft` never applies. `active` applies inside its window. The admin and
  `GET` show `draft`, `scheduled`, `active` or `expired`, derived like 0037's lists.
- **Codes** (`DiscountCode`, the code itself as the key):
  - `discount`, `code` (as typed, for display), the normalised code (trimmed, uppercase,
    Unicode NFKC; unique in the store: `keys::DISCOUNT_CODE`), `usage_limit:
    Option<u32>` (1 for single-use codes), `active`, `batch: Option<String>` (a label
    for a generated set).
  - A `Codes` discount has at least one code to be live.
- **Usage** (`discount_usage_counts`: `discount`, `used`; `discount_code_usage_counts`
  likewise; `discount_usages`: `discount`, `order`, `customer`, `code`) is written only
  by checkout's batch (§7).
- **Events:** `discount.created`, `discount.updated` (listing what changed),
  `discount.deleted`, `discount.codes_added`, `discount.codes_removed` (counts and
  batch, not every code).
  - Deleting a discount that orders used is allowed. Orders keep their copies (§8),
    as they do for products.
- **Scopes:** `discounts:read` and `discounts:write`, grantable to apps, keys and
  agents.

## 3. Conditions, targets, rewards and kinds (DS3, DS4)

```rust
pub enum Condition {
    Markets(Vec<MarketId>),
    Channels(Vec<ChannelId>),
    CustomerGroups(Vec<CustomerGroupId>),   // the buyer is in any
    Customers(Vec<CustomerId>),             // at most 1,000
    SignedIn,                               // a customer session (0034)
    MinSubtotal(PerMarket),                 // of the target items, after product discounts
    MinQuantity(u32),                       // of the target items
}

pub struct ItemSet {
    pub items: Items,          // All | Products(..) | Variants(..) | Categories(..)
    pub exclude_on_sale: bool, // 0037's `on_sale`: "not on already reduced items"
}

pub enum Value {
    Percent(BasisPoints),          // 1..=10000
    Amount { amounts: PerMarket, allocation: Allocation },
    FixedPrice(PerMarket),         // items only: "these for €5 each"
}

pub enum Allocation { Each, Across, Cheapest { units: u32 } }

pub enum Target { Items(ItemSet), Subtotal, Shipping { max_amount: Option<PerMarket> } }

pub struct Reward { pub target: Target, pub value: Value }

pub enum Kind {
    Standard { rewards: Vec<Reward> },     // 1..=3, one per class
    BuyXGetY {
        buys: ItemSet, buy: Threshold,     // Quantity(n) | Amount(PerMarket)
        gets: ItemSet, get_quantity: u32,
        value: Value,                      // Percent (10000 = free) or Amount Each
        uses_per_order: Option<u32>,
    },
    Tiered {
        items: ItemSet,
        measure: Measure,                  // Quantity | Subtotal
        tiers: Vec<Tier>,                  // up to 10, ascending thresholds, Percent or Amount
    },
}
```

- **Every list in a condition or item set** holds at most 1,000 IDs. Categories count
  their descendants, as in 0037.
- **Conditions are AND-ed; values inside a condition are OR-ed.** That covers every
  example in the survey's admin UIs. Nesting (any-of groups) is a later variant of the
  enum, added without reshaping the others.
- **`PerMarket`** is a map from market to `Money` in that market's currency, with at
  least one entry, checked against the markets' currencies when written
  (`discount_currency_mismatch`). A market that's deleted drops out of the map.
- **Buy X get Y:**
  - `buys` counts at full price, from items not already discounted by another
    combined discount (Shopify's rule).
  - The "gets" units are the cheapest qualifying units, taken from items other than
    the ones counted as bought when the sets overlap.
  - It repeats while the cart qualifies, up to `uses_per_order`.
  - The free item must be in the cart; the storefront hints when it isn't (§10).
- **Tiered** picks the highest tier whose threshold the target items reach, and applies
  its value to those items (`Across` for amounts). "Spend €100 save €15" is `Tiered
  {items: All, measure: Subtotal, tiers: [(100, 15)]}`, or a `Standard` subtotal reward
  with a `MinSubtotal` condition. The admin offers both, under the names "Volume" and
  "Amount off order".
- **Custom fields as targets** (`custom.material = wool`) are wanted and deferred. They
  need the values in the pricing load. The enum leaves room.

## 4. Combining (DS2)

- **Compatible pairs.** Discounts `a` and `b` can apply together when
  `a.combines_with ⊇ b.classes` and `b.combines_with ⊇ a.classes`.
  - Two product discounts that combine may both reduce the same line, applied in
    order (§5).
  - Two that don't combine still apply together when they touch different lines.
- **A candidate set is valid** when its members are pairwise compatible, it holds at
  most one shipping-class reward (one shipping discount per order, as Shopify does),
  and it holds every entered code that is in it.
- **Choosing the set** is deterministic and bounded:
  1. Candidates are the live automatic discounts whose cheap conditions (market,
     channel, groups, customer, signed in) hold, plus the cart's codes in the same
     state. There are at most 100 live automatic discounts (a constant,
     `MAX_LIVE_AUTOMATIC`; creating one more is `409 too_many_automatic_discounts`)
     and at most 5 codes.
  2. Each candidate's standalone saving is computed. Those saving nothing drop out,
     with their reason kept for the code states.
  3. For each candidate as a seed, the engine adds the others greedily, in descending
     standalone saving, when compatible with all chosen, then evaluates the set's real
     saving (§5).
  4. The best set wins. Ties go to the set with fewer discounts, then to the lowest
     IDs.
- **This is a heuristic.** Best-combination is a maximum-weight clique in general. It's
  exact whenever compatibility is "everything" or "nothing" (the cases merchants use),
  O(n²) evaluations otherwise, and documented as "best found by this rule".
  `reference`-style tests pin its choices.
- **An entered code that loses** stays on the cart with state
  `blocked_by_other {discount}`. The buyer sees "Can't be combined with Free shipping
  over €50" and can remove the code. Shopify rejects such a code at entry instead; we
  keep it because the cart may change.

## 5. The engine (DS5, DS6)

```rust
pub fn apply(input: &DiscountInput, discounts: &[Live<'_>]) -> Outcome
```

- **Pure:** no I/O, no clock (the input has `at`), deterministic, in `hypha-discounts`.
- **`DiscountInput`** is plain values built by the cart:
  - market, channel, currency, `taxes_included`;
  - the buyer (customer, groups, signed in);
  - lines (line index, product, variant, the product's categories with ancestors, unit
    price and compare-at from 0037, quantity, `on_sale`);
  - shipping groups with their chosen option's amount;
  - entered codes with their codes' rows.
- **`Outcome`:**
  - `applications` (discount, code, title, summary, the reward's value and target, total
    amount);
  - per-line allocations (application index, amount, and the units it covered for
    buy X get Y);
  - shipping allocations;
  - `code_states`.
- **The pipeline:**
  1. **Product rewards** on each line's amount from 0037 (unit price × quantity): its
     **base** for the class. Every product reward on the line is computed on that same
     base and the results are added, so two combined 10% and 20% discounts take 30% off.
     - `Each` amounts are per unit, × the covered units.
     - `Cheapest {units}` sorts the eligible units by unit price ascending (ties by line
       index) and covers the first `units`.
     - A fixed price reduces a unit to that price when it's lower, else nothing; its
       amount is the difference from the base.
     - **The cap.** When a line's rewards add up to more than its base, they're clipped
       in a fixed order (percentages, then amounts, then fixed prices; within each, by
       discount ID) so the line reaches exactly zero and the clipped rewards record what
       they actually took.
  2. **Order rewards** on the subtotal of their target lines *after* product rewards:
     that's the base for the class. `MinSubtotal` is checked against it too.
     - Each order reward computes its amount on that same base (a percentage rounded
       once, half away from zero), and the amounts add up: 10% and 20% on €100 take €30
       (Shopify's rule). They're capped at the base the same way as lines.
     - Each amount is then spread over its target lines by largest remainder.
  3. **Shipping rewards** on the chosen options' amounts (`max_amount` caps a "free
     shipping up to €10"). There's at most one, so nothing to add.
- **Why additive.** The total doesn't depend on the order discounts are listed in, a
  buyer reading "10% + 20%" gets 30%, and it's what Shopify merchants already expect.
  The price is that the cap is reachable (two 60% discounts on one line), so it's
  written as a rule, not an accident.
- **Rounding is per line, never per unit**, except `Each` amounts, which are exact by
  construction.
  - A line's percentage is `round(base × bps / 10000)`.
  - An order-level amount is computed once and then split to lines by largest
    remainder, weights being the lines' amounts after product rewards.
  - **Every allocation is an integer number of minor units**, and the applications'
    totals equal the sums of their allocations exactly. Unit tests pin halves at 0.5,
    a cent spread over three lines, a zero-amount line, and an amount larger than the
    pool (capped at the pool).
- **Tax.** The cart taxes each line's amount after allocations (`tax_on_net` /
  `tax_in_gross` unchanged, fed the discounted amount), and shipping likewise. In a
  tax-inclusive market a gross price minus a gross discount is the gross the tax is
  backed out of. Amounts are gross there by DS4, so there's no `is_tax_inclusive` flag
  to get wrong.
- **Shipping rates:**
  - Rates that depend on the subtotal (0015's "free over €X") see the subtotal **after**
    discounts, which is what the buyer pays.
  - The shipping options' read shows each option's amount before and after a shipping
    discount, so "Free" appears on the option.
- **Cost.** The engine runs on every cart read. For n candidates and m lines it does
  O(n² · m) arithmetic at the outside, microseconds for the caps above. Loading is one
  query for live automatic discounts in the market and channel, plus one for the
  cart's codes, pipelined with pricing's loads.
  *As built:* the live automatic discounts and the codes (then their discounts) are sent
  together; then the usage counts, the buyer's past uses (after the email's customer,
  when one is needed), the products' categories and the summaries' names together.

## 6. Codes and the cart (DS7)

- **`Cart.discount_codes: Vec<String>`** (normalised, at most 5).
- **Routes:**
  - `POST /storefront/carts/{id}/discount-codes {code}` adds a code.
    - An unknown, inactive or expired code is `422 discount_code_invalid`, the same for
      each so codes can't be enumerated by their errors, and isn't kept.
    - It's rate limited per client address and per cart (`Rule::DiscountCodeIp`,
      `DiscountCodeCart`): single-use codes are guessable.
  - `DELETE …/discount-codes/{code}` removes it.
- **A known code is kept even when it doesn't apply now**, with its state, so it applies
  once the cart qualifies. The states are:

  | State | Meaning |
  | --- | --- |
  | `applied` | in the winning set |
  | `not_eligible {condition}` | a condition fails (market, group, signed in…), named |
  | `minimum_not_met {needed}` | subtotal or quantity short, with what's missing |
  | `no_matching_items` | nothing in the cart is targeted |
  | `blocked_by_other {discount}` | a better combination excludes it |
  | `limit_reached` | total, per-code or per-customer use exhausted |
  | `not_available_in_market` | no amount for this market |

- **`PricedCart`** gains `discounts` (the applications with their summaries), each
  line's `allocations` and `total_after_discounts`, shipping allocations,
  `discount_total`, and `discount_codes` with their states.
- **UCP mapping.** A code's state becomes a `messages` entry with a `path` to the code
  (`$.discount_codes[1]`), and line allocations become per-line adjustments.
- **Signing in or out re-prices.** Groups and once-per-customer change with the buyer.
  It's the same read, so there's nothing to do.

## 7. Usage under concurrency (DS8)

- **Checkout runs the engine again** inside its retry loop, on the same loads as the
  cart.
- **For each application, it stages in the order's batch:**
  - `UPDATE discount_usage_counts SET used = used + 1 WHERE discount = $1 AND (limit IS
    NULL OR used < limit)` (and the code's counter likewise). No row changed is a
    `RepoError::Duplicate(keys::DISCOUNT_USAGE_LIMIT)`.
  - An insert into `discount_usages`, with a unique index on (discount, customer)
    when the discount is once per customer: `Duplicate(keys::DISCOUNT_PER_CUSTOMER)`.
- **The counter is a row apart from `discounts`**, so a popular automatic discount's
  concurrent checkouts don't fight over the discount's version (no `StaleVersion`
  retry storms). Unlimited discounts are counted too, for reports.
- **Losing a race.** Checkout maps either duplicate to `409 discounts_changed` with the
  cart re-priced in `details`: the code's new state, the new total. It doesn't place
  the order. A buyer never pays a different total than the one they last saw.
  - **Payment first (0031).** A started online payment is for the previous total;
    checkout's existing "the total changed" path releases or updates it, as it does
    for a price change.
- **"Once per customer"** keys on the customer, and customers are one per email
  (0005), so a guest is counted by their email too. Shopify does the same.
- **Cancelling an order doesn't give the use back** (Saleor, commercetools). A
  merchant who wants it back raises the limit. *Later:* an admin action "release this
  order's code".
- The in-memory `Table` gains a conditional counter so the contract scenario
  ("two checkouts race for the last use: one wins, one gets `Duplicate`") runs on both
  adapters.

## 8. Orders (DS9)

- **`Order.discounts: Vec<OrderDiscount>`**: index, discount ID (kept after the
  discount is deleted), code, title, summary, the reward's class, value and target,
  and amount.
- **`OrderLine` gains `discounts: Vec<Allocation { application: u16, amount: Money }>`**,
  and its fields become:
  - `subtotal` = `unit_price × quantity` (what `total` is today);
  - `discount_total` = the sum of its allocations;
  - `total` = `subtotal − discount_total`, which tax is computed on.
- **`OrderShippingLine`** likewise: `amount`, `discounts`, `total`.
- **`Order` gains `discount_total`.** `subtotal` stays the sum of the lines' totals
  after discounts (what the buyer is charged for goods), and the invariants are
  restated:
  - each line's `total = unit_price × quantity − Σ allocations ≥ 0`;
  - each application's amount = the sum of its allocations;
  - `discount_total` = the sum of the applications;
  - and the totals as today.
  - Restored orders from before this note have no discounts, and the same rule holds
    with zero.
- **This changes the meaning of `OrderLine.total` in the API** (it was before
  discounts). That's breaking, made before the first release, and labelled.
- **Refunds (later) use the line's prorated amount**: refunding k of n units refunds
  `total × k / n`, the remainder going to the last units refunded, so a line fully
  refunded returns exactly its `total`. The customer keeps the order discount's
  benefit on what they keep (Vendure's model). No redistribution: a refund never asks
  the buyer for money back on the remaining lines.
- **Emails:** the `order_placed` context gains `discounts` (title, code, amount
  formatted) and each line's `discount` / `original_total`, added and never renamed.
  The default templates show them.
- **Events:** `order.placed`'s payload gains `discount_codes` and `discount_total`
  (additive, same version).

## 9. API

- **Admin:**
  - `GET` / `POST /admin/discounts`. Filters: `status`, `method`, `class`, `q` (title
    or code), `code` (exact).
  - `GET` / `PATCH` / `DELETE /admin/discounts/{id}`.
  - `GET` / `POST /admin/discounts/{id}/codes` (add listed codes, up to 1,000 per call)
    and `POST …/codes/generate {count ≤ 10,000, prefix?, length 8..=24, batch?}`, in one
    batch. Generated codes use an alphabet without look-alikes (no `0/O/1/I/L`), and
    duplicates are drawn again.
  - `DELETE …/codes {codes | batch}`.
  - `GET …/usage` (paged usages with orders), `GET /admin/discounts/{id}/codes/export`
    (CSV).
- **Agents.** Every route is an operation in the index, and `POST /admin/discounts` is
  worth a curated MCP tool: "make a 10% welcome code" is a common admin ask. Writes go
  through preview and proposals like the rest.
- **Storefront:** the cart's code routes and DTO fields (§6). Product reads gain
  `promotions: [{title, summary}]`: the live automatic discounts that target the
  product and whose conditions involve only market and channel. It's a badge ("Buy 2,
  get 1 free"), never a price, so static pages stay correct for everyone.
- **Errors:**
  - `discount_code_invalid` (422), `discount_code_taken` (409, a duplicate),
    `discounts_changed` (409, checkout).
  - `too_many_automatic_discounts` (409), `discount_currency_mismatch` (422).
  - `invalid_discount` (422 with `details: {path, message}` for every rule broken,
    like the manifest checker).

## 10. The admin and the storefront

- **Navigation.** "Discounts" is a top-level item in the Store section.
- **The list:** title, method (codes count or "Automatic"), summary, status badge,
  window and uses.
- **"Create discount"** offers templates that fill the kind and classes:
  - Amount off products.
  - Amount off order.
  - Buy X get Y.
  - Volume (tiers).
  - Free shipping.
- **The form**, in sections:
  - Method (codes: one typed, or generate later; automatic: a title).
  - Value.
  - Applies to (item picker: products, variants, categories; exclude sale items).
  - Conditions (markets, channels, groups, customers, signed in, minimums).
  - Combinations (three checkboxes, with which live discounts each choice affects).
  - Limits.
  - Schedule.
  - A sticky **summary** panel shows `summary()` for the draft, read through a preview
    of the write.
- **A discount's page:** settings, a **Codes** tab (paged, search, add, generate,
  export, deactivate a batch), and **Usage** (orders, total discounted).
- **The order page** shows the applications under the totals and each line's
  allocation ("−€2.40 Welcome offer").
- **The storefront:**
  - The cart drawer and cart page get a code field, the applied codes with their
    states as messages, struck-through line totals with the discount's title, and a
    "Discounts" row in the totals.
  - Shipping options show "Free" when discounted.
  - Product pages show `promotions` badges.
  - The seed gains `WELCOME10`, "free shipping over €50", "buy a grow kit, get a syringe
    free" and a culture volume tier.

## 11. Apps and spores (reserved)

- **`Kind::External { provider }`** is reserved: an app's or a spore's discount logic.
  - It proposes candidates (targets and values, Shopify Functions' output shape), and
    the engine applies classes, combinations and allocation as for any other.
  - The pure engine is what makes this safe: providers propose, Hypha decides.
- **The open question is latency.**
  - Carts are priced on every read, so an HTTP call per read (an extension point
    `discounts.propose`) is too slow and too fragile for the hot path.
  - 0029 §11's wasm functions answer it, and a spore can implement a trait in-process
    today.
  - So the order is: a spore trait first (`DiscountProposer`, in `hypha-spore-core`),
    then wasm, then perhaps an HTTP point for checkout-time-only discounts.
  - None is built with this note.

## 12. Gift cards and store credit (sketch, own note next)

Sketched here so checkout is designed for it now:

- **They're payment, not discounts** (EU VAT directive 2016/1065: a multi-purpose
  voucher is taxed when spent, not when sold).
  - A gift card is sold as a product variant with no tax class and no shipping; buying
    it issues a card.
  - At checkout it pays part or all of the total **after** tax and discounts, and
    doesn't count toward discount minimums.
- **A new leaf, `hypha-gift-cards`:**
  - A card: `gc_…`, a code stored as SHA-256 with its last four characters, a currency,
    optional expiry (US law restricts it, so the default is none), the customer, and
    enabled.
  - An append-only ledger: issued, redeemed, refunded, adjusted, expired. The balance
    is a checked counter beside it, decremented in the order's batch like discount
    usage.
  - Lookups are rate limited like login codes.
  - Store credit is the same ledger, owned by a customer.
- **What checkout needs.**
  - **Split tender:** the order's payments can already be several (`financial_status`
    sums them), so a gift card is a `Payment` with provider `gift_card`, captured in
    the batch, and the online payment covers the rest.
  - Payment-first (0031) starts the online payment for the *remaining* amount. That's
    the change to plan for, and nothing in this note's build blocks it.

## 13. Layering

- `hypha-discounts` is a **leaf**: core only.
  - It names markets, channels, groups, customers, products, variants and categories
    by their core IDs.
  - It's handed categories, prices and groups as plain values, so it depends on none
    of those crates.
- `hypha-cart` (composing) gains `hypha-discounts`. It builds `DiscountInput` from the
  priced lines and runs the engine. `hypha-checkout` (a workflow) stages usage.
  - **This is a new edge, cart → discounts, to be approved.** It's the same shape as
    cart → pricing.
- Search doesn't index discounts (they're cart-level). `promotions` badges are read at
  request time from the discounts repo.

## 14. Hard to reverse, and still open

- **Classes and pairwise `combines_with` (DS2)**, and best-combination by the stated
  rule. That's a contract merchants learn.
- **Additive within a class (10% + 20% = 30%)**, Shopify's model (decided 2026-10-24).
- **Amounts per market (DS4)**, not per currency.
- **The order schema (§8):** applications, allocations, and `OrderLine.total` after
  discounts.
- **Usage not released on cancel (§7).**
- **Refunds prorated per line, no redistribution (§8).**
- **The cart → discounts crate edge (§13).**
- *Open:* should an invalid code be kept on the cart with a state (commercetools) or
  refused at entry (Shopify)? Proposed: unknown codes refused, known-but-not-applicable
  codes kept.

## 15. Build order

Each step is a commit series with tests, routes, OpenAPI, SDK and e2e steps. The engine
gets the most unit tests in the codebase: one rule each, every rounding half, every
state.

1. **The leaf:**
   - The model, conditions, rewards, `Standard` kind, codes, `summary()`, the engine for
     product, order and shipping rewards with allocation and combinations.
   - The repo, contract scenarios (the counter race included), migrations, events,
     scopes, admin CRUD and code routes.
2. **The cart:**
   - Codes on carts, `DiscountInput`, `PricedCart`'s discounts and states.
   - Tax on discounted amounts, shipping discounts and rates after discounts.
   - The storefront routes and rate limits.
3. **Checkout and orders:**
   - Applications and allocations on orders, the restated invariants and migration.
   - Usage in the batch, `discounts_changed`, payment-first's total change.
   - Emails' context, the `order.placed` fields.
4. **Buy X get Y and tiers.**
5. **The admin:** list, templates, form with summary, codes tab, usage, order page.
6. **The storefront:** code field, states, strikethroughs, totals row, "Free" shipping,
   promotion badges, the seed's discounts.
7. **Next note:** gift cards and store credit (§12), with split tender.

## As built

Steps 1 to 4 (the leaf and its engine, carts, checkout and orders, buy X get Y and tiers)
and the storefront's promotions. Where they differ from the sections above, or settle
what they left open:

- **The model** (§2, §3). Classes are derived from the kind, never given: a standard
  discount's are its rewards' targets'; buy X get Y and tiers are `product` (they reduce
  item lines). Each condition type appears at most once. Every rule broken is reported at
  once as `invalid_discount` with `details.problems: [{path, code, message}]` (`code`:
  `empty`, `too_many`, `duplicate`, `too_small`, `out_of_range`, `duplicate_class`,
  `allocation_not_allowed`, `value_not_allowed`, `threshold_mismatch`, `not_ascending`,
  `markets_differ`, `ends_not_after_start`, `invalid_text`). An amount for a market that
  doesn't exist is `422 market_not_found`, one in another currency `422
  discount_currency_mismatch` (both with `{path, market}`). The subtotal and shipping take
  only `across` amounts (the default when `allocation` is left out); a fixed price is for
  items only, and 0 makes them free. A tier's amount is across its items; spend tiers name
  the same markets, each rising. The method is fixed: `PATCH` refuses `method` as an
  unknown field (`422 invalid_body`); the domain's `discount_method_fixed` guards it too.
  A discount's products, variants, categories, customers, customer groups and channels are
  stored unchecked, like a payment method's markets (listed at the top of `hypha-http`'s
  `error.rs`): an unknown one never matches. Its markets are checked, since its amounts
  are priced in their currencies.
- **Storage.** A discount's conditions and kind are one JSON column (`rules`), `classes`
  beside it for the list's filter. A code's own use count is a column of its row
  (`discount_codes.used`), moved by the same conditional `UPDATE … WHERE used < limit` as
  the discount's counter row; neither has a version. Usage rows keep what the discount took
  off the order (`amount`) and aren't foreign keys to orders or customers (orders aren't
  deleted, nor customers with orders). Migrations 0083 (discounts), 0084 (carts' codes),
  0085 (orders' discounts), 0087 (usages by code).
  - **A claim is judged at the commit** (§7): the discount must still be live at the
    claim's moment, its row held `FOR SHARE` (concurrent claims don't wait on each other;
    an edit waits for them) so its limit can't change under the claim, and the code still
    active. A code deactivated mid-checkout is a lost claim: `409 discounts_changed`. The
    claim carries no limit of the discount's: storage uses the one the discount has then.
  - **Unlimited counters don't move.** Without a limit, neither the discount's counter row
    nor the code's `used` is written, so a popular unlimited discount or code is no row
    every checkout queues on. Their uses are counted from the usage rows instead (what
    `usage_counts` and a code's `used` answer; migration 0087 indexes them by code), and a
    discount given a limit later has its counter set from them in the same write (the
    write waits for claims in flight, so they're counted). A code's limit is fixed when
    it's made, so its claim's is the stored one.
- **The engine** (§4, §5).
  - A discount's minimum is measured on the qualifying items' prices before discounts when
    it reduces items, else on their amounts after product discounts.
  - Product discounts that don't combine still apply together when the lines each one
    reduces on its own don't overlap.
  - Several order rewards are each computed on the same subtotal, clipped as stated, then
    spread one after another over what each line has left (by largest remainder), so no
    line goes below zero.
  - Shipping is priced inside the engine, by a function the cart passes: the rate is for
    the shipped lines' subtotal after product and order discounts, and the shipping reward
    applies to that price. One pass, so "free over €50" and a 10% order discount agree.
  - Buy X get Y counts as bought the most expensive units still at full price (on lines no
    other discount in the set reduces), then gets the cheapest units left of `gets`; a
    round whose units aren't in the cart gets none. Shares on lines carry `units`.
  - Each set's members that took nothing are dropped before sets are compared.
  - **The lowest total wins** (the owner's decision, 2026-10-24; §4 step 4 said "saves
    most"). Sets are ranked by what the buyer pays, the lines and the shipping after
    discounts, not by what the discounts take off: a discount that takes the cart under a
    "free over €50" rate can make it cost more (5% off €52 is €49.40 plus €5 shipping).
    Compared are no discount at all, each candidate alone, and each seed's greedy set, so
    a discount never makes the cart cost more and a seed alone beats a combination that
    spoils it (a 10% shirt sale leaving a "buy anything, get the €5 sock free" deal
    nothing to count as bought: the deal alone saves €5, the two together €1). Ties go to
    fewer discounts, then lower IDs, so a discount that saves exactly what it adds to
    shipping isn't applied.
  - **Codes are taken in the order the cart entered them.** Two codes of one discount: the
    first entered applies, unless its own uses are taken and the other's aren't; the other
    reads its own state when it couldn't apply anyway (`limit_reached`), else
    `blocked_by_other` naming the discount.
- **Code states** (§6, §14). Unknown, inactive, draft and not-started codes are refused at
  entry as `422 discount_code_invalid`, alike. A kept code whose discount ended reads
  `expired`; one whose code or discount went (deleted, deactivated, made a draft) reads
  `invalid`. In the cart they're flat: `{code, state, condition?, needed?: {subtotal? |
  quantity?}, discount?}`. A second code of a discount already applied reads
  `blocked_by_other` naming that discount. A code left out though it saves on its own,
  by no discount it can't combine with, reads `no_saving`: with it the buyer would pay no
  less (the shipping case above). Generated codes are 8 to 24 characters with the prefix,
  at least 8 of them random after it (about 40 bits), so a prefix can't leave them
  guessable.
- **The buyer.** Signed in means the cart has a customer (a session's, or linked by staff).
  A guest with an email is counted by their email's customer for once-per-customer, as
  checkout will be (looked up only when a discount the cart weighs is once per customer);
  groups come only from a linked customer, as for prices, and so does being a listed
  customer: a guest typing a listed customer's email isn't them.
- **Carts** (§6). `subtotal` and `shipping_total` are after discounts; a line has
  `subtotal` (before), `discounts` and `total` (after, taxed); the chosen shipping
  `amount`, `discounts`, `total`; the cart `discounts`, `discount_total`,
  `discount_codes`. Shipping offers carry `amount_after_discounts`. An empty cart holding
  codes is read in full, so its codes get states. The routes answer the whole cart; rate
  limits `discount_code_ip` (10, then one every 6 s) and `discount_code_cart` (5, then one
  every 12 s).
- **Orders** (§7, §8). A lost claim is `409 discounts_changed` with `{total,
  discount_codes}` (the cart now) and isn't retried; money a payment committed goes back as
  on `total_changed`. An order's discounts keep their value and target as the leaf's JSON
  (`hypha_discounts::json`); Postgres's `jsonb` doesn't keep the keys' order.
  `order.placed`'s `discount_codes` names each code once, in the order applied. The
  emails' totals add up: the subtotal and shipping are after discounts, and the discounts
  are listed under the total as what they include ("Discounts included above"), never
  taken off again.
- **The admin's reads.** A discount's usage (`GET /admin/discounts/{id}/usage`) names
  orders and their customers, so it needs `customers:read` and `orders:read` besides
  `discounts:read`. The codes' CSV export checks the discount once and reads the codes
  5,000 at a time (`export_codes`, `DiscountRepo::codes_after`).
- **Summaries** (§10) name what they apply to ("Buy 1 from Grow kits, get 1 Liquid
  Culture Syringe free (once per order)", "Free shipping in Serbia on orders from 6,000
  RSD"; more than two names read "Grow kits and 2 more"), from `Names` the caller reads in
  one query (`DiscountRepo::names` over a `NameRequest`), counting whatever has no name
  ("2 categories"). Amounts are formatted as emails do (€, $ and £ before, other codes
  after, thousands grouped) without a fraction of zeros ("€50", "€12.50"), in the market
  asked for (else every market's, "€5 / 600 JPY"), and dates as UTC days; an end at
  midnight reads as the day before. Shoppers' summaries (carts, so orders, and
  promotions: `Audience::Shoppers`, `NameRequest::for_shoppers`) name only active
  products and never customers or groups ("for members", "for selected customers"); the
  admin's name customers only to a caller holding `customers:read` who may see names.
  Orders keep the sentence checkout rendered.
- **Promotions** (§9) are on storefront product reads with `country` and on search hits,
  `[{discount, title, summary}]`, read at request time: live automatic discounts that may
  reach the product (a variant list may reach any product), with no condition about the
  buyer. Static pages that show them follow discounts through `discount.created`,
  `discount.updated` and `discount.deleted`, which the storefront's webhook app should
  subscribe to.
- **Layering** (§13). The cart doesn't depend on discounts: it defines a port
  (`hypha_cart::promotions`: `HasPromotions`, `Promotions`, `PromotionReading`, in and out
  types of its own, value and target as the leaf's JSON) that the workflow
  `hypha-promotions` implements on the leaf (`CartPromotions`, `DiscountReading`); the
  storage adapters and the test world name it. Codes stay plain normalised strings on the
  cart, normalised and checked through the port; `MAX_CODES_PER_CART` is the cart's.
  Checkout stages usage with the leaf's `StageDiscounts` directly, from what each
  application carries (`once_per_customer`, the code's limit).
- **Agents.** `POST /admin/discounts` is a curated MCP tool (`admin__discounts__create`);
  every discount route is an operation.
