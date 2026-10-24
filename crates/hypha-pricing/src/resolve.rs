use std::collections::{HashMap, HashSet};

use hypha_core::{
    CategoryId, Currency, CustomerGroupId, Price, PriceListId, ProductId, Quantity, Timestamp,
    VariantId,
};
use hypha_markets::{
    Adjustment, Catalog, CompareAtMode, FixedPrice, Market, PriceBreak, PriceList, PriceListKind,
    PriceListPhase,
};

use crate::error::PricingError;

/// Everything needed to price a set of variants in one market, as plain data.
///
/// The caller fills it with batched lookups (`CatalogRepo::included_products`,
/// `PriceListRepo::fixed_prices`) for exactly the products and variants it is about
/// to price. Pricing itself stays a pure function: no I/O, easy to test.
///
/// A product or variant that wasn't asked for reads as "not in the catalog" or
/// "no fixed price", so build the context from everything you'll resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricingContext {
    pub market: Market,
    /// The market's catalog, if it has one.
    pub catalog: Option<Catalog>,
    /// The catalog's price lists that were found, live or not.
    pub price_lists: Vec<PriceList>,
    /// Which of the requested products the catalog includes.
    pub included_products: HashSet<ProductId>,
    /// Each live list's fixed prices for the requested variants that have one.
    pub fixed_prices: HashMap<PriceListId, HashMap<VariantId, FixedPrice>>,
    /// Each requested product's categories with all their ancestors: what an adjustment
    /// applying to categories reaches. Loaded only when a live list adjusts by category.
    pub product_categories: HashMap<ProductId, HashSet<CategoryId>>,
    /// The moment prices are for: a list prices only while its window is open then.
    pub at: Timestamp,
    /// The buyer's customer groups: a list for groups prices only for a buyer in one of
    /// them. Empty for a shopper who isn't signed in and for what every shopper shares
    /// (search documents, cached pages).
    pub groups: Vec<CustomerGroupId>,
}

impl PricingContext {
    /// A context that prices nothing in `market`: no catalog, so every product reads as
    /// not sold there. For a cart without lines, whose totals need only the market.
    pub fn empty(market: Market, at: Timestamp) -> Self {
        Self {
            market,
            catalog: None,
            price_lists: Vec::new(),
            included_products: HashSet::new(),
            fixed_prices: HashMap::new(),
            product_categories: HashMap::new(),
            at,
            groups: Vec::new(),
        }
    }
}

/// A variant's price for a shopper: what they pay, what it's compared against, and where
/// it came from. Carts, product pages and search all read it, so they show the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPrice {
    pub price: Price,
    /// Shown struck through beside `price`; only ever above it.
    pub compare_at: Option<Price>,
    /// The list whose price won; `None` for the base price.
    pub price_list: Option<PriceListId>,
    /// What larger lines cost each, for showing ("€8 each from 10"): every quantity above
    /// the one priced where the lowest offer of any list drops below the price before it.
    /// Empty when buying more costs no less.
    pub breaks: Vec<PriceBreak>,
}

impl ResolvedPrice {
    /// Whether a compare-at price is shown: what "on sale" means to a storefront.
    pub fn on_sale(&self) -> bool {
        self.compare_at.is_some()
    }
}

/// What a variant is priced from: its product, its own base and compare-at prices (in
/// its base currency).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariantPrices {
    pub product: ProductId,
    pub variant: VariantId,
    pub base: Price,
    pub compare_at: Option<Price>,
}

/// Why one of the catalog's lists did or didn't set a variant's price.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateResult {
    /// Its price is the one paid.
    Won,
    /// Another list's price was lower.
    Higher,
    /// Another list had the same price and is older.
    TiedWithOlder,
    /// Draft, not started yet or ended.
    NotLive(PriceListPhase),
    /// Live, but for customer groups the buyer isn't in.
    NotInAudience,
    /// No fixed price for the variant, and no adjustment covering its product.
    NotPricing,
}

/// One list's part in a variant's price, for explaining it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub price_list: PriceListId,
    /// What the list would charge; `None` unless it's live and prices the variant.
    pub price: Option<Price>,
    pub result: CandidateResult,
}

/// A variant's price with every list the catalog has: what the admin shows to explain
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceExplanation {
    pub resolved: Result<ResolvedPrice, PricingError>,
    /// In the catalog's order of lists; empty when the product isn't sold in the market.
    pub candidates: Vec<Candidate>,
}

/// One live list's offer for the variant.
struct Offer<'a> {
    list: &'a PriceList,
    price: Price,
    fixed: Option<&'a FixedPrice>,
    /// The list's adjustment, when it reaches the variant's product.
    adjustment: Option<&'a Adjustment>,
}

/// Price of a variant at `quantity` in the context's market, at the context's time.
///
/// 1. The market's catalog must include the product, otherwise it's not available there.
/// 2. Every live list on the catalog whose audience includes the buyer and that prices
///    the variant (a fixed price for it, or an adjustment reaching its product) offers
///    its price for `quantity` (its best break
///    at or below it, else its fixed price, else the base price adjusted); the lowest
///    wins, a tie going to the oldest list.
/// 3. With no offer, the base price is the price.
///
/// The compare-at price follows the winner's kind: a `sale` list is measured against the
/// regular price (the lowest `override` offer, else the base price), an `override` list
/// against the variant's compare-at (adjusted, or hidden, as the list says), and the base
/// price against the variant's compare-at. A fixed price's own compare-at beats either.
/// It's shown only when it's above the price.
///
/// Every returned price is in the market's currency. A base price in another currency is
/// an error when it's needed, until exchange rates exist.
pub fn resolve_price(
    pricing: &PricingContext,
    variant: VariantPrices,
    quantity: Quantity,
) -> Result<ResolvedPrice, PricingError> {
    evaluate(pricing, variant, quantity).map(|(resolved, _)| resolved)
}

/// `resolve_price` with each of the catalog's lists and what became of it.
pub fn explain_price(
    pricing: &PricingContext,
    variant: VariantPrices,
    quantity: Quantity,
) -> PriceExplanation {
    match evaluate(pricing, variant, quantity) {
        Ok((resolved, candidates)) => PriceExplanation {
            resolved: Ok(resolved),
            candidates,
        },
        Err(e) => PriceExplanation {
            resolved: Err(e),
            candidates: Vec::new(),
        },
    }
}

fn evaluate(
    pricing: &PricingContext,
    variant: VariantPrices,
    quantity: Quantity,
) -> Result<(ResolvedPrice, Vec<Candidate>), PricingError> {
    let market = &pricing.market;
    let catalog = pricing
        .catalog
        .as_ref()
        .filter(|_| pricing.included_products.contains(&variant.product))
        .ok_or(PricingError::NotAvailable {
            product: variant.product,
            market: market.id(),
        })?;

    let mut candidates = Vec::new();
    let mut offers: Vec<Offer> = Vec::new();
    let no_categories = HashSet::new();
    let categories = pricing
        .product_categories
        .get(&variant.product)
        .unwrap_or(&no_categories);
    for &list_id in catalog.price_lists() {
        let list = pricing
            .price_lists
            .iter()
            .find(|l| l.id() == list_id)
            .ok_or(PricingError::PriceListNotFound(list_id))?;
        let phase = list.phase(pricing.at);
        if phase != PriceListPhase::Active {
            candidates.push(Candidate {
                price_list: list_id,
                price: None,
                result: CandidateResult::NotLive(phase),
            });
            continue;
        }
        if !list.audience().includes(&pricing.groups) {
            candidates.push(Candidate {
                price_list: list_id,
                price: None,
                result: CandidateResult::NotInAudience,
            });
            continue;
        }
        let fixed = pricing
            .fixed_prices
            .get(&list_id)
            .and_then(|prices| prices.get(&variant.variant));
        let adjustment = list
            .adjustment()
            .filter(|a| a.covers(variant.product, categories));
        let price = match (fixed, adjustment) {
            (Some(fixed), _) => Some(fixed.price_at(quantity.value())),
            // A base price in another currency, or one the adjustment takes out of range,
            // is something this list can't price: the others (or the base price) still can.
            (None, Some(adjustment)) => (variant.base.money().currency() == market.currency())
                .then(|| variant.base.money().adjust(adjustment.percent()).ok())
                .flatten()
                .and_then(|m| Price::new(m).ok()),
            (None, None) => None,
        };
        let Some(price) = price else {
            candidates.push(Candidate {
                price_list: list_id,
                price: None,
                result: CandidateResult::NotPricing,
            });
            continue;
        };
        // A fixed price is in the list's currency, so the list itself has to match the
        // market.
        if list.currency() != market.currency() {
            return Err(PricingError::CurrencyMismatch {
                expected: market.currency(),
                found: list.currency(),
            });
        }
        candidates.push(Candidate {
            price_list: list_id,
            price: Some(price),
            // Settled once the winner is known.
            result: CandidateResult::Higher,
        });
        offers.push(Offer {
            list,
            price,
            fixed,
            adjustment,
        });
    }

    let Some(winner) = offers
        .iter()
        .min_by_key(|o| (o.price.money().amount(), o.list.id()))
    else {
        ensure_currency(market.currency(), variant.base)?;
        let resolved = ResolvedPrice {
            price: variant.base,
            compare_at: shown(variant.compare_at, variant.base),
            price_list: None,
            breaks: Vec::new(),
        };
        return Ok((resolved, candidates));
    };

    for candidate in &mut candidates {
        if candidate.price_list == winner.list.id() {
            candidate.result = CandidateResult::Won;
        } else if candidate.price == Some(winner.price) {
            candidate.result = CandidateResult::TiedWithOlder;
        }
    }

    let compare_at = match winner.fixed.and_then(FixedPrice::compare_at) {
        Some(own) => Some(own),
        None => match winner.list.kind() {
            PriceListKind::Sale => regular_price(&offers, variant.base, market.currency()),
            PriceListKind::Override => match winner.list.compare_at() {
                CompareAtMode::Hidden => None,
                CompareAtMode::Adjusted => variant
                    .compare_at
                    .filter(|c| c.money().currency() == market.currency())
                    .and_then(|c| adjusted(c, winner.adjustment)),
            },
        },
    };
    let resolved = ResolvedPrice {
        price: winner.price,
        compare_at: compare_at.and_then(|c| shown(Some(c), winner.price)),
        price_list: Some(winner.list.id()),
        breaks: ladder(&offers, quantity.value(), winner.price),
    };
    Ok((resolved, candidates))
}

/// The unit prices a buyer of more than `quantity` would pay, across every offer: at each
/// quantity some offer's break starts, the lowest offer there, kept where it's below the
/// price before it. What's shown is then what a larger line is charged, whichever list
/// wins it.
fn ladder(offers: &[Offer], quantity: u32, price: Price) -> Vec<PriceBreak> {
    let mut quantities: Vec<u32> = offers
        .iter()
        .filter_map(|o| o.fixed)
        .flat_map(|f| f.breaks().iter().map(|b| b.min_quantity))
        .filter(|&q| q > quantity)
        .collect();
    quantities.sort_unstable();
    quantities.dedup();
    let mut last = price;
    let mut ladder = Vec::new();
    for q in quantities {
        let Some(lowest) = offers
            .iter()
            .map(|o| o.fixed.map_or(o.price, |f| f.price_at(q)))
            .min_by_key(|p| p.money().amount())
        else {
            continue;
        };
        if lowest.money().amount() < last.money().amount() {
            ladder.push(PriceBreak {
                min_quantity: q,
                price: lowest,
            });
            last = lowest;
        }
    }
    ladder
}

/// What a sale is measured against: the lowest `override` offer, else the base price
/// (when it's in the market's currency; otherwise there's nothing to compare with).
fn regular_price(offers: &[Offer], base: Price, currency: Currency) -> Option<Price> {
    offers
        .iter()
        .filter(|o| o.list.kind() == PriceListKind::Override)
        .map(|o| o.price)
        .min_by_key(|p| p.money().amount())
        .or_else(|| (base.money().currency() == currency).then_some(base))
}

/// The variant's compare-at with the list's adjustment applied, when it reaches the
/// product; as it is otherwise. `None` when adjusting overflows, rather than failing the
/// price.
fn adjusted(compare_at: Price, adjustment: Option<&Adjustment>) -> Option<Price> {
    match adjustment {
        Some(adjustment) => compare_at
            .money()
            .adjust(adjustment.percent())
            .ok()
            .and_then(|m| Price::new(m).ok()),
        None => Some(compare_at),
    }
}

/// `compare_at` when it's above `price`, in the same currency: nothing else is struck
/// through.
fn shown(compare_at: Option<Price>, price: Price) -> Option<Price> {
    compare_at.filter(|c| {
        c.money().currency() == price.money().currency()
            && c.money().amount() > price.money().amount()
    })
}

fn ensure_currency(expected: Currency, price: Price) -> Result<(), PricingError> {
    let found = price.money().currency();
    if found != expected {
        Err(PricingError::CurrencyMismatch { expected, found })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
