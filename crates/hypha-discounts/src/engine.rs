//! Applying discounts to a cart, to the cent: a pure function of plain values the cart
//! builds (no I/O, no clock), so the cart, checkout and tests all get the same answer.
//!
//! - **Product** rewards reduce lines, each computed on the line's own amount (its base)
//!   and added up; a line's rewards past its base are clipped (percentages first, then
//!   amounts, then fixed prices, each by discount ID) so it reaches exactly zero.
//! - **Order** rewards are computed on the subtotal after product rewards, added up and
//!   clipped the same way, then spread over the lines by largest remainder.
//! - **Shipping**: at most one, on the chosen option's price for the subtotal after
//!   product and order rewards.
//!
//! Which discounts apply together is chosen by a stated rule: each candidate seeds a set
//! the others join greedily (largest standalone saving first) when compatible with every
//! member; the set that saves most wins, ties going to fewer discounts, then lower IDs.

use std::collections::{BTreeMap, HashSet};

use hypha_core::{
    CategoryId, ChannelId, Currency, CustomerGroupId, CustomerId, DiscountId, MarketId, Money,
    ProductId, Timestamp, VariantId,
};

use crate::{
    discount::{
        Allocation, Class, Condition, Discount, ItemSet, Kind, Measure, PerMarket, Phase, Reward,
        Target, Threshold, Value,
    },
    summary::{Audience, Names, summary},
};

/// Who is buying.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Buyer {
    /// For a guest, the customer their email belongs to, if any: what once-per-customer
    /// counts by.
    pub customer: Option<CustomerId>,
    pub groups: Vec<CustomerGroupId>,
    pub signed_in: bool,
}

/// An item line, priced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub product: ProductId,
    pub variant: VariantId,
    /// The product's categories with all their ancestors.
    pub categories: HashSet<CategoryId>,
    /// What one unit costs before discounts, in the input's currency.
    pub unit_price: Money,
    pub quantity: u32,
    /// Shown against a compare-at price.
    pub on_sale: bool,
}

impl Line {
    fn base(&self) -> i64 {
        self.unit_price.amount() * i64::from(self.quantity)
    }

    fn in_set(&self, set: &ItemSet) -> bool {
        set.contains(self.product, self.variant, &self.categories, self.on_sale)
    }
}

/// A group of lines shipped together, with the chosen option's price.
pub struct ShippingGroup<'a> {
    /// The input lines it ships (their indexes).
    pub lines: Vec<usize>,
    /// The chosen option's price for a subtotal of its lines after product and order
    /// discounts: rates that depend on the subtotal see what the buyer pays. `None` when
    /// nothing is chosen or it can't be priced.
    pub rate: &'a dyn Fn(Money) -> Option<Money>,
}

/// What the engine applies discounts to.
pub struct DiscountInput<'a> {
    pub at: Timestamp,
    pub market: MarketId,
    pub channel: ChannelId,
    pub currency: Currency,
    pub buyer: Buyer,
    /// The lines that can be bought, in cart order.
    pub lines: Vec<Line>,
    pub shipping: Vec<ShippingGroup<'a>>,
    /// The codes the cart holds, normalised, in the order they were entered: each gets a
    /// state.
    pub codes: Vec<String>,
    /// What the discounts' summaries call what they name (`Names::none()` to count).
    pub names: &'a Names,
}

/// A discount that may apply: a live automatic one, or the discount of a code the cart
/// holds.
#[derive(Debug, Clone, Copy)]
pub struct Live<'a> {
    pub discount: &'a Discount,
    /// The code (normalised) it came by; `None` for an automatic one.
    pub code: Option<&'a str>,
    /// Its uses, its code's, or the buyer's one use per customer are taken.
    pub limit_reached: bool,
}

/// What's missing for a minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needed {
    /// This much more of the qualifying items.
    Amount(Money),
    /// This many more qualifying units.
    Quantity(u32),
}

/// Where a code the cart holds stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeState {
    /// In the discounts applied.
    Applied,
    /// A condition fails (`markets`, `channels`, `customer_groups`, `customers`,
    /// `signed_in`).
    NotEligible { condition: &'static str },
    /// The qualifying items fall short.
    MinimumNotMet { needed: Needed },
    /// Nothing in the cart is reduced by it.
    NoMatchingItems,
    /// A combination that saves more leaves it out; `discount` is one that excludes it.
    BlockedByOther { discount: DiscountId },
    /// Its uses are taken (in all, the code's own, or once per customer).
    LimitReached,
    /// It has no amount for the cart's market.
    NotAvailableInMarket,
    /// Its discount's end date passed.
    Expired,
    /// The code or its discount is gone, deactivated, a draft, or not started yet.
    Invalid,
}

impl CodeState {
    pub fn name(self) -> &'static str {
        match self {
            CodeState::Applied => "applied",
            CodeState::NotEligible { .. } => "not_eligible",
            CodeState::MinimumNotMet { .. } => "minimum_not_met",
            CodeState::NoMatchingItems => "no_matching_items",
            CodeState::BlockedByOther { .. } => "blocked_by_other",
            CodeState::LimitReached => "limit_reached",
            CodeState::NotAvailableInMarket => "not_available_in_market",
            CodeState::Expired => "expired",
            CodeState::Invalid => "invalid",
        }
    }
}

/// One reward applied: what it was and how much it took in all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Application {
    pub discount: DiscountId,
    /// The code it came by, normalised.
    pub code: Option<String>,
    pub title: String,
    /// `summary` in the cart's market.
    pub summary: String,
    pub class: Class,
    /// The reward's value: a standard reward's, a buy X get Y's, or the tier reached.
    pub value: Value,
    /// What it reduced: a standard reward's target, the items got, or the tiered items.
    pub target: Target,
    pub amount: Money,
}

/// Part of an application on one line or shipping group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Share {
    /// Its index in `Outcome::applications`.
    pub application: usize,
    pub amount: Money,
    /// The units it covered, for a buy X get Y discount.
    pub units: Option<u32>,
}

/// A shipping group after discounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShippingOutcome {
    /// Its lines' amount after product and order discounts: what the rate was priced for.
    pub subtotal: Money,
    /// The chosen option's price for that subtotal, before shipping discounts.
    pub amount: Option<Money>,
    pub shares: Vec<Share>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeOutcome {
    pub code: String,
    pub state: CodeState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Product rewards, then order, then shipping; each class by discount ID.
    pub applications: Vec<Application>,
    /// Per input line, its shares.
    pub lines: Vec<Vec<Share>>,
    /// Per input shipping group.
    pub shipping: Vec<ShippingOutcome>,
    /// Per code held, in order.
    pub codes: Vec<CodeOutcome>,
    /// Everything taken off: the sum of the applications.
    pub total: Money,
}

/// Applies the best set of `candidates` to `input`.
pub fn apply(input: &DiscountInput<'_>, candidates: &[Live<'_>]) -> Outcome {
    // One candidate per discount: a second code of the same discount adds nothing.
    let mut seen = HashSet::new();
    let unique: Vec<Live<'_>> = candidates
        .iter()
        .copied()
        .filter(|c| seen.insert(c.discount.id()))
        .collect();

    let checked: Vec<Option<CodeState>> = unique.iter().map(|c| pre_check(input, c)).collect();
    let mut standalone: Vec<Option<Evaluation>> = vec![None; unique.len()];
    for (i, state) in checked.iter().enumerate() {
        if state.is_none() {
            standalone[i] = Some(evaluate(input, &unique, &[i]));
        }
    }
    let mut viable: Vec<usize> = (0..unique.len())
        .filter(|i| standalone[*i].as_ref().is_some_and(|e| e.saving > 0))
        .collect();
    viable.sort_by(|a, b| {
        let (sa, sb) = (saving(&standalone, *a), saving(&standalone, *b));
        sb.cmp(&sa)
            .then_with(|| unique[*a].discount.id().cmp(&unique[*b].discount.id()))
    });

    let touched = |i: usize| -> HashSet<usize> {
        standalone[i]
            .as_ref()
            .map(Evaluation::touched_lines)
            .unwrap_or_default()
    };
    let compatible = |a: usize, b: usize| -> bool {
        let (da, db) = (unique[a].discount, unique[b].discount);
        if da.classes().contains(Class::Shipping) && db.classes().contains(Class::Shipping) {
            return false;
        }
        if da.combines(db) {
            return true;
        }
        // Product discounts that don't combine still apply together on different lines.
        let product_only = |d: &Discount| d.classes().classes() == [Class::Product];
        product_only(da) && product_only(db) && touched(a).is_disjoint(&touched(b))
    };

    let mut best: Option<Evaluation> = None;
    for seed in &viable {
        let mut set = vec![*seed];
        for other in &viable {
            if other != seed && set.iter().all(|m| compatible(*m, *other)) {
                set.push(*other);
            }
        }
        let evaluation = evaluate(input, &unique, &set);
        if better(&evaluation, best.as_ref(), &unique) {
            best = Some(evaluation);
        }
    }
    let best = best.unwrap_or_else(|| evaluate(input, &unique, &[]));

    let codes = input
        .codes
        .iter()
        .map(|code| CodeOutcome {
            code: code.clone(),
            state: code_state(
                code,
                candidates,
                &unique,
                &checked,
                &standalone,
                &best,
                &compatible,
            ),
        })
        .collect();
    best.into_outcome(input, &unique, codes)
}

fn saving(standalone: &[Option<Evaluation>], i: usize) -> i64 {
    standalone[i].as_ref().map_or(0, |e| e.saving)
}

/// Whether `candidate` beats `best`: saves more, or as much with fewer discounts, or as
/// many with lower IDs.
fn better(candidate: &Evaluation, best: Option<&Evaluation>, unique: &[Live<'_>]) -> bool {
    let Some(best) = best else {
        return true;
    };
    let ids = |e: &Evaluation| {
        let mut ids: Vec<DiscountId> = e.members.iter().map(|m| unique[*m].discount.id()).collect();
        ids.sort();
        ids
    };
    candidate
        .saving
        .cmp(&best.saving)
        .then_with(|| best.members.len().cmp(&candidate.members.len()))
        .then_with(|| ids(best).cmp(&ids(candidate)))
        .is_gt()
}

#[allow(clippy::too_many_arguments)]
fn code_state(
    code: &str,
    candidates: &[Live<'_>],
    unique: &[Live<'_>],
    checked: &[Option<CodeState>],
    standalone: &[Option<Evaluation>],
    best: &Evaluation,
    compatible: &dyn Fn(usize, usize) -> bool,
) -> CodeState {
    let Some(entered) = candidates.iter().find(|c| c.code == Some(code)) else {
        return CodeState::Invalid;
    };
    let Some(i) = unique
        .iter()
        .position(|c| c.discount.id() == entered.discount.id())
    else {
        return CodeState::Invalid;
    };
    // A second code of a discount that came by another code.
    if unique[i].code != Some(code) {
        return CodeState::BlockedByOther {
            discount: unique[i].discount.id(),
        };
    }
    if let Some(state) = checked[i] {
        return state;
    }
    if best.members.contains(&i) {
        return CodeState::Applied;
    }
    if let Some(evaluation) = &standalone[i]
        && evaluation.saving == 0
    {
        return evaluation
            .reasons
            .get(&i)
            .copied()
            .unwrap_or(CodeState::NoMatchingItems);
    }
    let mut winners: Vec<usize> = best.members.clone();
    winners.sort_by_key(|m| unique[*m].discount.id());
    let blocker = winners
        .iter()
        .find(|w| !compatible(**w, i))
        .or(winners.first())
        .map(|w| unique[*w].discount.id());
    match blocker {
        Some(discount) => CodeState::BlockedByOther { discount },
        None => CodeState::NoMatchingItems,
    }
}

/// Why a candidate can't apply to this cart whatever else applies: not live, a condition
/// about the buyer, no amount in the market, its uses taken.
fn pre_check(input: &DiscountInput<'_>, candidate: &Live<'_>) -> Option<CodeState> {
    let d = candidate.discount;
    match d.phase(input.at) {
        Phase::Active => {}
        Phase::Expired => return Some(CodeState::Expired),
        Phase::Draft | Phase::Scheduled => return Some(CodeState::Invalid),
    }
    for condition in d.conditions() {
        let holds = match condition {
            Condition::Markets(ids) => ids.contains(&input.market),
            Condition::Channels(ids) => ids.contains(&input.channel),
            Condition::CustomerGroups(ids) => ids.iter().any(|g| input.buyer.groups.contains(g)),
            // Only a signed-in (or staff-linked) customer: a guest typing a listed
            // customer's email isn't them. The email's customer only counts uses.
            Condition::Customers(ids) => {
                input.buyer.signed_in && input.buyer.customer.is_some_and(|c| ids.contains(&c))
            }
            Condition::SignedIn => input.buyer.signed_in,
            Condition::MinSubtotal(_) | Condition::MinQuantity(_) => true,
        };
        if !holds {
            return Some(CodeState::NotEligible {
                condition: condition.name(),
            });
        }
    }
    if !has_amounts_in(d, input.market) {
        return Some(CodeState::NotAvailableInMarket);
    }
    if candidate.limit_reached {
        return Some(CodeState::LimitReached);
    }
    None
}

/// Whether every amount the discount needs has an entry for `market`.
fn has_amounts_in(d: &Discount, market: MarketId) -> bool {
    let has = |p: &PerMarket| p.get(market).is_some();
    let conditions = d.conditions().iter().all(|c| match c {
        Condition::MinSubtotal(min) => has(min),
        _ => true,
    });
    let kind = match d.kind() {
        Kind::Standard { rewards } => rewards.iter().all(|r| {
            r.value.amounts().is_none_or(has)
                && match &r.target {
                    Target::Shipping {
                        max_amount: Some(max),
                    } => has(max),
                    _ => true,
                }
        }),
        Kind::BuyXGetY { buy, value, .. } => {
            value.amounts().is_none_or(has)
                && match buy {
                    Threshold::Amount(a) => has(a),
                    Threshold::Quantity(_) => true,
                }
        }
        Kind::Tiered { tiers, .. } => tiers.iter().all(|t| {
            t.value.amounts().is_none_or(has)
                && match &t.threshold {
                    Threshold::Amount(a) => has(a),
                    Threshold::Quantity(_) => true,
                }
        }),
    };
    conditions && kind
}

/// A reward as the engine works it: which candidate and which of its rewards (a standard
/// discount's index; 0 for the other kinds), and the value and target it reports.
#[derive(Debug, Clone)]
struct RawApplication {
    candidate: usize,
    reward: usize,
    class: Class,
    value: Value,
    target: Target,
}

/// How clipping orders rewards: percentages, then amounts, then fixed prices.
fn rank(value: &Value) -> u8 {
    match value {
        Value::Percent(_) => 0,
        Value::Amount { .. } => 1,
        Value::FixedPrice(_) => 2,
    }
}

/// A shipping group in an evaluation: its subtotal, its price, and (application, amount)
/// shares.
type GroupShares = (i64, Option<i64>, Vec<(usize, i64)>);

/// A set of discounts applied.
#[derive(Debug, Clone)]
struct Evaluation {
    /// The candidates that took something off.
    members: Vec<usize>,
    applications: Vec<RawApplication>,
    /// What each application took in all.
    totals: Vec<i64>,
    /// Per line: (application, amount, units).
    lines: Vec<Vec<(usize, i64, Option<u32>)>>,
    shipping: Vec<GroupShares>,
    saving: i64,
    /// Why a member took nothing, when it's a minimum.
    reasons: BTreeMap<usize, CodeState>,
}

impl Evaluation {
    fn touched_lines(&self) -> HashSet<usize> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, shares)| shares.iter().any(|(_, amount, _)| *amount > 0))
            .map(|(i, _)| i)
            .collect()
    }

    fn into_outcome(
        self,
        input: &DiscountInput<'_>,
        unique: &[Live<'_>],
        codes: Vec<CodeOutcome>,
    ) -> Outcome {
        let money = |amount: i64| {
            // Every amount is a part of a price the cart already fits.
            Money::new(amount, input.currency).unwrap_or(Money::zero(input.currency))
        };
        // Order: class, then discount ID, then reward; applications that took nothing go.
        let mut order: Vec<usize> = (0..self.applications.len())
            .filter(|a| self.totals[*a] > 0)
            .collect();
        order.sort_by(|a, b| {
            let (x, y) = (&self.applications[*a], &self.applications[*b]);
            x.class
                .cmp(&y.class)
                .then_with(|| {
                    unique[x.candidate]
                        .discount
                        .id()
                        .cmp(&unique[y.candidate].discount.id())
                })
                .then_with(|| x.reward.cmp(&y.reward))
        });
        let index_of = |raw: usize| order.iter().position(|a| *a == raw);
        let applications: Vec<Application> = order
            .iter()
            .map(|a| {
                let raw = &self.applications[*a];
                let live = unique[raw.candidate];
                Application {
                    discount: live.discount.id(),
                    code: live.code.map(str::to_string),
                    title: live.discount.title().to_string(),
                    summary: summary(
                        live.discount,
                        Some(input.market),
                        input.names,
                        Audience::Shoppers,
                    ),
                    class: raw.class,
                    value: raw.value.clone(),
                    target: raw.target.clone(),
                    amount: money(self.totals[*a]),
                }
            })
            .collect();
        let lines = self
            .lines
            .iter()
            .map(|shares| {
                let mut out: Vec<Share> = shares
                    .iter()
                    .filter(|(_, amount, _)| *amount > 0)
                    .filter_map(|(a, amount, units)| {
                        Some(Share {
                            application: index_of(*a)?,
                            amount: money(*amount),
                            units: *units,
                        })
                    })
                    .collect();
                out.sort_by_key(|s| s.application);
                out
            })
            .collect();
        let shipping = self
            .shipping
            .iter()
            .map(|(subtotal, amount, shares)| ShippingOutcome {
                subtotal: money(*subtotal),
                amount: amount.map(money),
                shares: shares
                    .iter()
                    .filter(|(_, amount)| *amount > 0)
                    .filter_map(|(a, amount)| {
                        Some(Share {
                            application: index_of(*a)?,
                            amount: money(*amount),
                            units: None,
                        })
                    })
                    .collect(),
            })
            .collect();
        let total = applications
            .iter()
            .fold(0i64, |sum, a| sum + a.amount.amount());
        Outcome {
            applications,
            lines,
            shipping,
            codes,
            total: money(total),
        }
    }
}

/// `round(amount × bp / 10000)`, half away from zero; `amount` is never negative.
fn percent_of(amount: i64, bp: i32) -> i64 {
    let n = i128::from(amount) * i128::from(bp);
    let mut q = n / 10_000;
    if (n % 10_000) * 2 >= 10_000 {
        q += 1;
    }
    q as i64
}

/// Splits `total` over `weights` in proportion, to the unit: each share is its exact
/// part rounded down, and the units left go to the largest fractional parts, ties to the
/// earlier index, so the shares sum to `total` exactly. `total` is at most the weights'
/// sum, so no share passes its weight; with no weight, nothing is shared.
pub fn spread(total: i64, weights: &[i64]) -> Vec<i64> {
    let sum: i128 = weights.iter().map(|w| i128::from(*w)).sum();
    if sum <= 0 || total <= 0 {
        return vec![0; weights.len()];
    }
    let total = i128::from(total).min(sum);
    let mut shares: Vec<i64> = Vec::with_capacity(weights.len());
    let mut fractions: Vec<(i128, usize)> = Vec::with_capacity(weights.len());
    for (i, w) in weights.iter().enumerate() {
        let exact = total * i128::from(*w);
        shares.push((exact / sum) as i64);
        fractions.push((exact % sum, i));
    }
    let given: i128 = shares.iter().map(|s| i128::from(*s)).sum();
    let left = (total - given) as usize;
    fractions.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, i) in fractions.into_iter().take(left) {
        shares[i] += 1;
    }
    shares
}

/// Which lines a discount's conditions measure: its item targets', or every line when it
/// reduces no items.
fn qualifying(d: &Discount, lines: &[Line]) -> Vec<usize> {
    let sets: Vec<&ItemSet> = match d.kind() {
        Kind::Standard { rewards } => rewards
            .iter()
            .filter_map(|r| match &r.target {
                Target::Items(set) => Some(set),
                _ => None,
            })
            .collect(),
        Kind::BuyXGetY { buys, .. } => vec![buys],
        Kind::Tiered { items, .. } => vec![items],
    };
    (0..lines.len())
        .filter(|i| sets.is_empty() || sets.iter().any(|s| lines[*i].in_set(s)))
        .collect()
}

/// Whether the minimums hold on `amounts` (each line's amount the minimum is measured
/// on), or what's missing.
fn minimums(d: &Discount, input: &DiscountInput<'_>, amounts: &[i64]) -> Result<(), CodeState> {
    let lines = qualifying(d, &input.lines);
    for condition in d.conditions() {
        match condition {
            Condition::MinSubtotal(min) => {
                let Some(min) = min.get(input.market) else {
                    return Err(CodeState::NotAvailableInMarket);
                };
                let have: i64 = lines.iter().map(|i| amounts[*i]).sum();
                if have < min.amount() {
                    let short = Money::new(min.amount() - have, input.currency)
                        .unwrap_or(Money::zero(input.currency));
                    return Err(CodeState::MinimumNotMet {
                        needed: Needed::Amount(short),
                    });
                }
            }
            Condition::MinQuantity(n) => {
                let have: u64 = lines
                    .iter()
                    .map(|i| u64::from(input.lines[*i].quantity))
                    .sum();
                if have < u64::from(*n) {
                    return Err(CodeState::MinimumNotMet {
                        needed: Needed::Quantity((u64::from(*n) - have) as u32),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// One proposed reduction of a line before clipping.
struct Proposal {
    application: usize,
    line: usize,
    amount: i64,
    units: Option<u32>,
}

/// Applies `set` (indexes into `candidates`) to the input.
fn evaluate(input: &DiscountInput<'_>, candidates: &[Live<'_>], set: &[usize]) -> Evaluation {
    let lines = &input.lines;
    let bases: Vec<i64> = lines.iter().map(Line::base).collect();
    let mut members: Vec<usize> = set.to_vec();
    members.sort_by_key(|m| candidates[*m].discount.id());
    let mut applications: Vec<RawApplication> = Vec::new();
    let mut reasons = BTreeMap::new();

    // Who passes their minimums: discounts reducing items measure the prices before
    // discounts; the others, the amounts after product discounts (below).
    let mut product_members = Vec::new();
    let mut later_members = Vec::new();
    for m in &members {
        let d = candidates[*m].discount;
        if d.classes().contains(Class::Product) {
            match minimums(d, input, &bases) {
                Ok(()) => product_members.push(*m),
                Err(state) => {
                    reasons.insert(*m, state);
                }
            }
        } else {
            later_members.push(*m);
        }
    }

    // ---- product ----
    let mut proposals: Vec<Proposal> = Vec::new();
    // Standard and tiered first: buy X get Y counts what they leave undiscounted.
    for m in &product_members {
        let d = candidates[*m].discount;
        match d.kind() {
            Kind::Standard { rewards } => {
                for (r, reward) in rewards.iter().enumerate() {
                    let Target::Items(set) = &reward.target else {
                        continue;
                    };
                    let application = applications.len();
                    applications.push(RawApplication {
                        candidate: *m,
                        reward: r,
                        class: Class::Product,
                        value: reward.value.clone(),
                        target: reward.target.clone(),
                    });
                    let eligible: Vec<usize> =
                        (0..lines.len()).filter(|i| lines[*i].in_set(set)).collect();
                    item_proposals(input, reward, &eligible, application, &mut proposals);
                }
            }
            Kind::Tiered {
                items,
                measure,
                tiers,
            } => {
                let eligible: Vec<usize> = (0..lines.len())
                    .filter(|i| lines[*i].in_set(items))
                    .collect();
                let reached = match measure {
                    Measure::Quantity => {
                        let units: u64 =
                            eligible.iter().map(|i| u64::from(lines[*i].quantity)).sum();
                        tiers.iter().rev().find(|t| match t.threshold {
                            Threshold::Quantity(n) => units >= u64::from(n),
                            Threshold::Amount(_) => false,
                        })
                    }
                    Measure::Subtotal => {
                        let spent: i64 = eligible.iter().map(|i| bases[*i]).sum();
                        tiers.iter().rev().find(|t| match &t.threshold {
                            Threshold::Amount(a) => {
                                a.get(input.market).is_some_and(|min| spent >= min.amount())
                            }
                            Threshold::Quantity(_) => false,
                        })
                    }
                };
                let Some(tier) = reached else {
                    // Short of the first tier: a minimum not met.
                    if let Some(first) = tiers.first() {
                        let needed = match &first.threshold {
                            Threshold::Quantity(n) => {
                                let units: u64 =
                                    eligible.iter().map(|i| u64::from(lines[*i].quantity)).sum();
                                Needed::Quantity((u64::from(*n).saturating_sub(units)) as u32)
                            }
                            Threshold::Amount(a) => {
                                let spent: i64 = eligible.iter().map(|i| bases[*i]).sum();
                                let min = a.get(input.market).map_or(0, |m| m.amount());
                                Needed::Amount(
                                    Money::new((min - spent).max(0), input.currency)
                                        .unwrap_or(Money::zero(input.currency)),
                                )
                            }
                        };
                        if !eligible.is_empty() {
                            reasons.insert(*m, CodeState::MinimumNotMet { needed });
                        }
                    }
                    continue;
                };
                let application = applications.len();
                let target = Target::Items(items.clone());
                applications.push(RawApplication {
                    candidate: *m,
                    reward: 0,
                    class: Class::Product,
                    value: tier.value.clone(),
                    target: target.clone(),
                });
                let reward = Reward {
                    target,
                    value: tier.value.clone(),
                };
                item_proposals(input, &reward, &eligible, application, &mut proposals);
            }
            Kind::BuyXGetY { .. } => {}
        }
    }
    for m in &product_members {
        let d = candidates[*m].discount;
        let Kind::BuyXGetY {
            buys,
            buy,
            gets,
            get_quantity,
            value,
            uses_per_order,
        } = d.kind()
        else {
            continue;
        };
        let discounted: HashSet<usize> = proposals
            .iter()
            .filter(|p| p.amount > 0)
            .map(|p| p.line)
            .collect();
        let application = applications.len();
        applications.push(RawApplication {
            candidate: *m,
            reward: 0,
            class: Class::Product,
            value: value.clone(),
            target: Target::Items(gets.clone()),
        });
        let got = buy_x_get_y(
            input,
            buys,
            buy,
            gets,
            *get_quantity,
            *uses_per_order,
            &discounted,
        );
        for (line, units) in got.iter().enumerate() {
            if *units == 0 {
                continue;
            }
            let unit = lines[line].unit_price.amount();
            let amount = match value {
                Value::Percent(bp) => percent_of(unit * i64::from(*units), bp.value()),
                Value::Amount { amounts, .. } => {
                    let off = amounts.get(input.market).map_or(0, |a| a.amount());
                    off.min(unit) * i64::from(*units)
                }
                Value::FixedPrice(_) => 0,
            };
            proposals.push(Proposal {
                application,
                line,
                amount,
                units: Some(*units),
            });
        }
    }

    // Clip each line at its base: percentages, then amounts, then fixed prices, each by
    // discount ID, then reward.
    let mut line_shares: Vec<Vec<(usize, i64, Option<u32>)>> = vec![Vec::new(); lines.len()];
    proposals.sort_by(|a, b| {
        let (x, y) = (&applications[a.application], &applications[b.application]);
        rank(&x.value)
            .cmp(&rank(&y.value))
            .then_with(|| {
                candidates[x.candidate]
                    .discount
                    .id()
                    .cmp(&candidates[y.candidate].discount.id())
            })
            .then_with(|| x.reward.cmp(&y.reward))
            .then_with(|| a.line.cmp(&b.line))
    });
    let mut left: Vec<i64> = bases.clone();
    for p in &proposals {
        let take = p.amount.min(left[p.line]).max(0);
        left[p.line] -= take;
        line_shares[p.line].push((p.application, take, p.units));
    }

    // ---- order ----
    let mut order_rewards: Vec<(usize, i64)> = Vec::new();
    let after_product = left.clone();
    for m in &members {
        let d = candidates[*m].discount;
        let Kind::Standard { rewards } = d.kind() else {
            continue;
        };
        let applies = if product_members.contains(m) {
            true
        } else if later_members.contains(m) {
            match minimums(d, input, &after_product) {
                Ok(()) => true,
                Err(state) => {
                    reasons.insert(*m, state);
                    false
                }
            }
        } else {
            false
        };
        if !applies {
            continue;
        }
        for (r, reward) in rewards.iter().enumerate() {
            if reward.target != Target::Subtotal {
                continue;
            }
            let base: i64 = after_product.iter().sum();
            let amount = match &reward.value {
                Value::Percent(bp) => percent_of(base, bp.value()),
                Value::Amount { amounts, .. } => amounts
                    .get(input.market)
                    .map_or(0, |a| a.amount())
                    .min(base),
                Value::FixedPrice(_) => 0,
            };
            let application = applications.len();
            applications.push(RawApplication {
                candidate: *m,
                reward: r,
                class: Class::Order,
                value: reward.value.clone(),
                target: reward.target.clone(),
            });
            order_rewards.push((application, amount));
        }
    }
    order_rewards.sort_by(|a, b| {
        let (x, y) = (&applications[a.0], &applications[b.0]);
        rank(&x.value)
            .cmp(&rank(&y.value))
            .then_with(|| {
                candidates[x.candidate]
                    .discount
                    .id()
                    .cmp(&candidates[y.candidate].discount.id())
            })
            .then_with(|| x.reward.cmp(&y.reward))
    });
    let mut pool: i64 = after_product.iter().sum();
    for (application, amount) in order_rewards {
        let take = amount.min(pool).max(0);
        pool -= take;
        // Spread over what each line has left, so no line goes below zero however many
        // order rewards share it.
        let shares = spread(take, &left);
        for (line, share) in shares.into_iter().enumerate() {
            if share > 0 {
                left[line] -= share;
                line_shares[line].push((application, share, None));
            }
        }
    }

    // ---- shipping ----
    let mut shipping = Vec::with_capacity(input.shipping.len());
    for group in &input.shipping {
        let subtotal: i64 = group.lines.iter().filter_map(|i| left.get(*i)).sum();
        let price = Money::new(subtotal, input.currency)
            .ok()
            .and_then(|s| (group.rate)(s))
            .filter(|p| p.currency() == input.currency)
            .map(|p| p.amount().max(0));
        shipping.push((subtotal, price, Vec::new()));
    }
    for m in &members {
        let d = candidates[*m].discount;
        let Kind::Standard { rewards } = d.kind() else {
            continue;
        };
        if reasons.contains_key(m) || !(product_members.contains(m) || later_members.contains(m)) {
            continue;
        }
        for (r, reward) in rewards.iter().enumerate() {
            let Target::Shipping { max_amount } = &reward.target else {
                continue;
            };
            let prices: Vec<i64> = shipping.iter().map(|(_, p, _)| p.unwrap_or(0)).collect();
            let charged: i64 = prices.iter().sum();
            let mut amount = match &reward.value {
                Value::Percent(bp) => percent_of(charged, bp.value()),
                Value::Amount { amounts, .. } => {
                    amounts.get(input.market).map_or(0, |a| a.amount())
                }
                Value::FixedPrice(_) => 0,
            }
            .min(charged);
            if let Some(max) = max_amount.as_ref().and_then(|m| m.get(input.market)) {
                amount = amount.min(max.amount());
            }
            let application = applications.len();
            applications.push(RawApplication {
                candidate: *m,
                reward: r,
                class: Class::Shipping,
                value: reward.value.clone(),
                target: reward.target.clone(),
            });
            for (g, share) in spread(amount, &prices).into_iter().enumerate() {
                if share > 0 {
                    shipping[g].2.push((application, share));
                }
            }
        }
    }

    let mut totals = vec![0i64; applications.len()];
    for shares in &line_shares {
        for (a, amount, _) in shares {
            totals[*a] += amount;
        }
    }
    for (_, _, shares) in &shipping {
        for (a, amount) in shares {
            totals[*a] += amount;
        }
    }
    let saving = totals.iter().sum();
    let contributed: HashSet<usize> = applications
        .iter()
        .enumerate()
        .filter(|(a, _)| totals[*a] > 0)
        .map(|(_, raw)| raw.candidate)
        .collect();
    members.retain(|m| contributed.contains(m));
    Evaluation {
        members,
        applications,
        totals,
        lines: line_shares,
        shipping,
        saving,
        reasons,
    }
}

/// A standard item reward's (or a tier's) reductions of the `eligible` lines, before
/// clipping.
fn item_proposals(
    input: &DiscountInput<'_>,
    reward: &Reward,
    eligible: &[usize],
    application: usize,
    proposals: &mut Vec<Proposal>,
) {
    let lines = &input.lines;
    let mut push = |line: usize, amount: i64| {
        if amount > 0 {
            proposals.push(Proposal {
                application,
                line,
                amount,
                units: None,
            });
        }
    };
    match &reward.value {
        Value::Percent(bp) => {
            for i in eligible {
                push(*i, percent_of(lines[*i].base(), bp.value()));
            }
        }
        Value::Amount {
            amounts,
            allocation,
        } => {
            let off = amounts.get(input.market).map_or(0, |a| a.amount());
            match allocation {
                Allocation::Each => {
                    for i in eligible {
                        let unit = lines[*i].unit_price.amount();
                        push(*i, off.min(unit) * i64::from(lines[*i].quantity));
                    }
                }
                Allocation::Across => {
                    let weights: Vec<i64> = eligible.iter().map(|i| lines[*i].base()).collect();
                    let pool: i64 = weights.iter().sum();
                    for (i, share) in eligible.iter().zip(spread(off.min(pool), &weights)) {
                        push(*i, share);
                    }
                }
                Allocation::Cheapest { units } => {
                    let covered = cheapest_units(lines, eligible, *units);
                    for (i, k) in covered {
                        let unit = lines[i].unit_price.amount();
                        push(i, off.min(unit) * i64::from(k));
                    }
                }
            }
        }
        Value::FixedPrice(prices) => {
            let Some(price) = prices.get(input.market) else {
                return;
            };
            for i in eligible {
                let unit = lines[*i].unit_price.amount();
                if unit > price.amount() {
                    push(*i, (unit - price.amount()) * i64::from(lines[*i].quantity));
                }
            }
        }
    }
}

/// The cheapest `units` units among `eligible` lines (by unit price, ties to the earlier
/// line), as (line, units covered).
fn cheapest_units(lines: &[Line], eligible: &[usize], units: u32) -> Vec<(usize, u32)> {
    let mut by_price: Vec<usize> = eligible.to_vec();
    by_price.sort_by(|a, b| {
        lines[*a]
            .unit_price
            .amount()
            .cmp(&lines[*b].unit_price.amount())
            .then(a.cmp(b))
    });
    let mut left = units;
    let mut covered = Vec::new();
    for i in by_price {
        if left == 0 {
            break;
        }
        let k = lines[i].quantity.min(left);
        left -= k;
        covered.push((i, k));
    }
    covered
}

/// How many units of each line a buy X get Y discount gets, repeating while the cart
/// qualifies. Each round counts the most expensive units of `buys` still free (full
/// price, not on a line another discount reduces) as bought until `buy` is met, then
/// gets the `get_quantity` cheapest units of `gets` not counted yet; a round that can't
/// get them all gets none.
fn buy_x_get_y(
    input: &DiscountInput<'_>,
    buys: &ItemSet,
    buy: &Threshold,
    gets: &ItemSet,
    get_quantity: u32,
    uses_per_order: Option<u32>,
    discounted: &HashSet<usize>,
) -> Vec<u32> {
    let lines = &input.lines;
    let mut used: Vec<u32> = vec![0; lines.len()];
    let mut got: Vec<u32> = vec![0; lines.len()];
    let buy_lines: Vec<usize> = {
        let mut v: Vec<usize> = (0..lines.len())
            .filter(|i| lines[*i].in_set(buys) && !discounted.contains(i))
            .collect();
        // Most expensive first, ties to the earlier line.
        v.sort_by(|a, b| {
            lines[*b]
                .unit_price
                .amount()
                .cmp(&lines[*a].unit_price.amount())
                .then(a.cmp(b))
        });
        v
    };
    let get_lines: Vec<usize> = {
        let mut v: Vec<usize> = (0..lines.len())
            .filter(|i| lines[*i].in_set(gets))
            .collect();
        v.sort_by(|a, b| {
            lines[*a]
                .unit_price
                .amount()
                .cmp(&lines[*b].unit_price.amount())
                .then(a.cmp(b))
        });
        v
    };
    let mut rounds = 0u32;
    loop {
        if uses_per_order.is_some_and(|max| rounds >= max) {
            break;
        }
        // Count as bought, most expensive first.
        let mut trial = used.clone();
        let mut met = false;
        let (mut units, mut spent) = (0u64, 0i64);
        'buying: for i in &buy_lines {
            while trial[*i] < lines[*i].quantity {
                trial[*i] += 1;
                units += 1;
                spent += lines[*i].unit_price.amount();
                met = match buy {
                    Threshold::Quantity(n) => units >= u64::from(*n),
                    Threshold::Amount(a) => {
                        a.get(input.market).is_some_and(|min| spent >= min.amount())
                    }
                };
                if met {
                    break 'buying;
                }
            }
        }
        if !met {
            break;
        }
        // Get the cheapest units left.
        let mut wanted = get_quantity;
        let mut taken = Vec::new();
        for i in &get_lines {
            while wanted > 0 && trial[*i] < lines[*i].quantity {
                trial[*i] += 1;
                wanted -= 1;
                taken.push(*i);
            }
        }
        if wanted > 0 {
            break;
        }
        for i in taken {
            got[i] += 1;
        }
        used = trial;
        rounds += 1;
    }
    got
}

#[cfg(test)]
mod tests;
