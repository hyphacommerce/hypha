use hypha_core::{BasisPoints, CatalogId, CategoryId, CountryCode, MarketId, Money};
use hypha_markets::{AppliesTo, Audience, CatalogContext, PriceListDraft, PriceListStatus};
use uuid::Uuid;

use super::*;

fn vid() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(2))
}

fn mid() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(3))
}

fn plid(n: u128) -> PriceListId {
    PriceListId::from_uuid(Uuid::from_u128(100 + n))
}

fn pid() -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(1))
}

fn eur(amount: i64) -> Price {
    Price::new(Money::new(amount, Currency::EUR).unwrap()).unwrap()
}

fn rsd(amount: i64) -> Price {
    Price::new(Money::new(amount, Currency::RSD).unwrap()).unwrap()
}

fn at(micros: i64) -> Timestamp {
    Timestamp::from_unix_micros(micros)
}

fn market(currency: Currency) -> Market {
    Market::new(
        mid(),
        "Test".to_string(),
        vec![CountryCode::new("RS").unwrap()],
        currency,
    )
    .unwrap()
}

/// An active `override` list, adjusting by `bp` when given.
fn list(n: u128, currency: Currency, bp: Option<i32>) -> PriceList {
    let draft = PriceListDraft::new(&format!("List {n}"), currency);
    let draft = match bp {
        Some(bp) => draft.adjusted(BasisPoints::new(bp).unwrap()),
        None => draft,
    };
    PriceList::new(plid(n), draft).unwrap()
}

fn sale(n: u128, bp: Option<i32>) -> PriceList {
    let mut l = list(n, Currency::EUR, bp);
    l.set_kind(PriceListKind::Sale);
    l
}

/// The product is in the market's catalog; the catalog uses `price_list` if given.
fn context(market_currency: Currency, price_list: Option<PriceList>) -> PricingContext {
    PricingContext {
        market: market(market_currency),
        catalog: Some(Catalog::new(
            CatalogId::from_uuid(Uuid::from_u128(5)),
            CatalogContext::Market(mid()),
            price_list.as_ref().map(|l| l.id()),
        )),
        price_lists: price_list.into_iter().collect(),
        included_products: HashSet::from([pid()]),
        fixed_prices: HashMap::new(),
        product_categories: HashMap::new(),
        at: at(1_000),
        groups: Vec::new(),
    }
}

/// `context` with every list of `lists` on the catalog.
fn with_lists(lists: Vec<PriceList>) -> PricingContext {
    let mut ctx = context(Currency::EUR, None);
    ctx.catalog = Some(Catalog::new(
        CatalogId::from_uuid(Uuid::from_u128(5)),
        CatalogContext::Market(mid()),
        lists.iter().map(|l| l.id()),
    ));
    ctx.price_lists = lists;
    ctx
}

fn scoped(n: u128, bp: i32, applies_to: AppliesTo) -> PriceList {
    let mut l = list(n, Currency::EUR, None);
    l.set_adjustment(Some(
        Adjustment::new(BasisPoints::new(bp).unwrap(), applies_to).unwrap(),
    ));
    l
}

fn cat(n: u128) -> CategoryId {
    CategoryId::from_uuid(Uuid::from_u128(200 + n))
}

fn fix(ctx: &mut PricingContext, list: PriceListId, fixed: FixedPrice) {
    ctx.fixed_prices
        .entry(list)
        .or_default()
        .insert(vid(), fixed);
}

fn variant(base: Price) -> VariantPrices {
    VariantPrices {
        product: pid(),
        variant: vid(),
        base,
        compare_at: None,
    }
}

fn was(base: Price, compare_at: Price) -> VariantPrices {
    VariantPrices {
        compare_at: Some(compare_at),
        ..variant(base)
    }
}

fn one() -> Quantity {
    Quantity::ONE
}

fn price_of(ctx: &PricingContext, v: VariantPrices) -> Result<Price, PricingError> {
    resolve_price(ctx, v, one()).map(|r| r.price)
}

fn compare_at_of(ctx: &PricingContext, v: VariantPrices) -> Option<Price> {
    resolve_price(ctx, v, one()).unwrap().compare_at
}

#[test]
fn uses_base_price_without_price_list() {
    let ctx = context(Currency::EUR, None);
    assert_eq!(
        resolve_price(&ctx, variant(eur(1000)), one()),
        Ok(ResolvedPrice {
            price: eur(1000),
            compare_at: None,
            price_list: None,
            breaks: vec![],
        })
    );
}

#[test]
fn fixed_price_wins_over_adjustment() {
    let mut ctx = context(Currency::EUR, Some(list(1, Currency::EUR, Some(-5000))));
    fix(&mut ctx, plid(1), FixedPrice::plain(eur(777)));
    assert_eq!(
        resolve_price(&ctx, variant(eur(1000)), one()),
        Ok(ResolvedPrice {
            price: eur(777),
            compare_at: None,
            price_list: Some(plid(1)),
            breaks: vec![],
        })
    );
}

#[test]
fn adjusts_base_price_without_fixed_price() {
    let ctx = context(Currency::EUR, Some(list(1, Currency::EUR, Some(-1000))));
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(900)));
}

#[test]
fn adjusting_rounds_half_away_from_zero() {
    // 1005 × 0.9 = 904.5.
    let ctx = context(Currency::EUR, Some(list(1, Currency::EUR, Some(-1000))));
    assert_eq!(price_of(&ctx, variant(eur(1005))), Ok(eur(905)));
}

#[test]
fn a_list_without_adjustment_prices_only_its_fixed_variants() {
    let ctx = context(Currency::EUR, Some(list(1, Currency::EUR, None)));
    assert_eq!(
        resolve_price(&ctx, variant(eur(1000)), one()).map(|r| (r.price, r.price_list)),
        Ok((eur(1000), None))
    );
}

#[test]
fn a_draft_list_prices_nothing() {
    let mut draft = list(1, Currency::EUR, Some(-1000));
    draft.set_status(PriceListStatus::Draft);
    let ctx = context(Currency::EUR, Some(draft));
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(1000)));
}

fn windowed(starts: Option<i64>, ends: Option<i64>) -> PriceList {
    let mut l = list(1, Currency::EUR, Some(-1000));
    l.set_window(hypha_markets::Window::new(starts.map(at), ends.map(at)).unwrap());
    l
}

#[test]
fn a_list_prices_from_the_moment_its_window_opens() {
    let mut ctx = context(Currency::EUR, Some(windowed(Some(1_000), None)));
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(900)));
    ctx.at = at(999);
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(1000)));
}

#[test]
fn a_list_stops_pricing_the_moment_its_window_closes() {
    let mut ctx = context(Currency::EUR, Some(windowed(None, Some(1_001))));
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(900)));
    ctx.at = at(1_001);
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(1000)));
}

#[test]
fn rejects_market_without_catalog() {
    let ctx = PricingContext {
        catalog: None,
        ..context(Currency::EUR, None)
    };
    assert_eq!(
        price_of(&ctx, variant(eur(1000))),
        Err(PricingError::NotAvailable {
            product: pid(),
            market: mid(),
        })
    );
}

#[test]
fn rejects_product_missing_from_catalog() {
    let ctx = PricingContext {
        included_products: HashSet::new(),
        ..context(Currency::EUR, None)
    };
    assert_eq!(
        price_of(&ctx, variant(eur(1000))),
        Err(PricingError::NotAvailable {
            product: pid(),
            market: mid(),
        })
    );
}

#[test]
fn rejects_missing_price_list() {
    let ctx = PricingContext {
        price_lists: Vec::new(),
        ..context(Currency::EUR, Some(list(1, Currency::EUR, Some(0))))
    };
    assert_eq!(
        price_of(&ctx, variant(eur(1000))),
        Err(PricingError::PriceListNotFound(plid(1)))
    );
}

#[test]
fn rejects_base_price_in_other_currency_without_price_list() {
    let ctx = context(Currency::RSD, None);
    assert_eq!(
        price_of(&ctx, variant(eur(1000))),
        Err(PricingError::CurrencyMismatch {
            expected: Currency::RSD,
            found: Currency::EUR,
        })
    );
}

#[test]
fn rejects_base_price_in_other_currency_without_fixed_price() {
    let ctx = context(Currency::RSD, Some(list(1, Currency::RSD, Some(0))));
    assert_eq!(
        price_of(&ctx, variant(eur(1000))),
        Err(PricingError::CurrencyMismatch {
            expected: Currency::RSD,
            found: Currency::EUR,
        })
    );
}

#[test]
fn fixed_price_covers_base_price_in_other_currency() {
    let mut ctx = context(Currency::RSD, Some(list(1, Currency::RSD, Some(0))));
    fix(&mut ctx, plid(1), FixedPrice::plain(rsd(117_000)));
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(rsd(117_000)));
}

#[test]
fn rejects_price_list_in_other_currency_than_market() {
    let mut ctx = context(Currency::RSD, Some(list(1, Currency::EUR, Some(0))));
    fix(&mut ctx, plid(1), FixedPrice::plain(eur(900)));
    assert_eq!(
        price_of(&ctx, variant(eur(1000))),
        Err(PricingError::CurrencyMismatch {
            expected: Currency::RSD,
            found: Currency::EUR,
        })
    );
}

// ---------- compare-at prices ----------

#[test]
fn the_base_price_shows_the_variants_compare_at() {
    let ctx = context(Currency::EUR, None);
    assert_eq!(
        compare_at_of(&ctx, was(eur(1000), eur(1500))),
        Some(eur(1500))
    );
}

#[test]
fn a_compare_at_not_above_the_price_is_not_shown() {
    let mut ctx = context(Currency::EUR, Some(list(1, Currency::EUR, None)));
    fix(&mut ctx, plid(1), FixedPrice::plain(eur(1500)));
    let resolved = resolve_price(&ctx, was(eur(1000), eur(1500)), one()).unwrap();
    assert_eq!((resolved.price, resolved.compare_at), (eur(1500), None));
    assert!(!resolved.on_sale());
}

#[test]
fn a_sale_list_is_compared_against_the_base_price() {
    let ctx = context(Currency::EUR, Some(sale(1, Some(-2000))));
    let resolved = resolve_price(&ctx, variant(eur(1000)), one()).unwrap();
    assert_eq!(
        (resolved.price, resolved.compare_at),
        (eur(800), Some(eur(1000)))
    );
    assert!(resolved.on_sale());
}

#[test]
fn a_sale_lists_own_compare_at_wins() {
    let mut ctx = context(Currency::EUR, Some(sale(1, None)));
    fix(
        &mut ctx,
        plid(1),
        FixedPrice::new(eur(700), Some(eur(1200))).unwrap(),
    );
    assert_eq!(compare_at_of(&ctx, variant(eur(1000))), Some(eur(1200)));
}

#[test]
fn a_sale_price_above_the_regular_price_shows_no_compare_at() {
    let mut ctx = context(Currency::EUR, Some(sale(1, None)));
    fix(&mut ctx, plid(1), FixedPrice::plain(eur(1100)));
    assert_eq!(compare_at_of(&ctx, variant(eur(1000))), None);
}

#[test]
fn an_override_list_adjusts_the_variants_compare_at() {
    let ctx = context(Currency::EUR, Some(list(1, Currency::EUR, Some(1000))));
    let resolved = resolve_price(&ctx, was(eur(1000), eur(1500)), one()).unwrap();
    assert_eq!(
        (resolved.price, resolved.compare_at),
        (eur(1100), Some(eur(1650)))
    );
}

#[test]
fn an_override_list_hides_the_compare_at_when_it_says_so() {
    let mut hidden = list(1, Currency::EUR, Some(1000));
    hidden.set_compare_at(CompareAtMode::Hidden);
    let ctx = context(Currency::EUR, Some(hidden));
    assert_eq!(compare_at_of(&ctx, was(eur(1000), eur(1500))), None);
}

#[test]
fn an_override_lists_own_compare_at_shows_even_when_hidden() {
    let mut hidden = list(1, Currency::EUR, None);
    hidden.set_compare_at(CompareAtMode::Hidden);
    let mut ctx = context(Currency::EUR, Some(hidden));
    fix(
        &mut ctx,
        plid(1),
        FixedPrice::new(eur(900), Some(eur(1300))).unwrap(),
    );
    assert_eq!(
        compare_at_of(&ctx, was(eur(1000), eur(1500))),
        Some(eur(1300))
    );
}

#[test]
fn an_override_list_shows_no_compare_at_in_another_currency() {
    let mut ctx = context(Currency::RSD, Some(list(1, Currency::RSD, None)));
    fix(&mut ctx, plid(1), FixedPrice::plain(rsd(117_000)));
    assert_eq!(compare_at_of(&ctx, was(eur(1000), eur(1500))), None);
}

// ---------- explaining ----------

#[test]
fn explains_each_list() {
    let mut draft = list(2, Currency::EUR, Some(-5000));
    draft.set_status(PriceListStatus::Draft);
    let mut ctx = context(Currency::EUR, Some(list(1, Currency::EUR, Some(-1000))));
    ctx.price_lists.push(draft);
    let explained = explain_price(&ctx, variant(eur(1000)), one());
    assert_eq!(
        explained.candidates,
        [Candidate {
            price_list: plid(1),
            price: Some(eur(900)),
            result: CandidateResult::Won,
        }]
    );
}

#[test]
fn explains_a_list_that_prices_nothing() {
    let ctx = context(Currency::EUR, Some(list(1, Currency::EUR, None)));
    let explained = explain_price(&ctx, variant(eur(1000)), one());
    assert_eq!(
        explained.candidates,
        [Candidate {
            price_list: plid(1),
            price: None,
            result: CandidateResult::NotPricing,
        }]
    );
    assert_eq!(explained.resolved.map(|r| r.price), Ok(eur(1000)));
}

#[test]
fn explains_a_list_that_is_not_live() {
    let mut draft = list(1, Currency::EUR, Some(-1000));
    draft.set_status(PriceListStatus::Draft);
    let ctx = context(Currency::EUR, Some(draft));
    assert_eq!(
        explain_price(&ctx, variant(eur(1000)), one()).candidates,
        [Candidate {
            price_list: plid(1),
            price: None,
            result: CandidateResult::NotLive(PriceListPhase::Draft),
        }]
    );
}

// ---------- several lists ----------

#[test]
fn the_lowest_live_price_wins() {
    let ctx = with_lists(vec![
        list(1, Currency::EUR, Some(-1000)),
        list(2, Currency::EUR, Some(-3000)),
        list(3, Currency::EUR, Some(500)),
    ]);
    assert_eq!(
        resolve_price(&ctx, variant(eur(1000)), one()).map(|r| (r.price, r.price_list)),
        Ok((eur(700), Some(plid(2))))
    );
}

#[test]
fn the_base_price_competes_only_without_a_list_pricing_the_variant() {
    // A markup list is the only one pricing it: the shopper pays more than the base price.
    let ctx = with_lists(vec![
        list(1, Currency::EUR, Some(2000)),
        list(2, Currency::EUR, None),
    ]);
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(1200)));
}

#[test]
fn a_tie_goes_to_the_oldest_list() {
    let mut ctx = with_lists(vec![
        list(2, Currency::EUR, None),
        list(1, Currency::EUR, Some(-1000)),
    ]);
    fix(&mut ctx, plid(2), FixedPrice::plain(eur(900)));
    assert_eq!(
        resolve_price(&ctx, variant(eur(1000)), one()).map(|r| r.price_list),
        Ok(Some(plid(1)))
    );
    assert_eq!(
        explain_price(&ctx, variant(eur(1000)), one()).candidates,
        [
            Candidate {
                price_list: plid(1),
                price: Some(eur(900)),
                result: CandidateResult::Won,
            },
            Candidate {
                price_list: plid(2),
                price: Some(eur(900)),
                result: CandidateResult::TiedWithOlder,
            },
        ]
    );
}

#[test]
fn a_higher_list_loses() {
    let ctx = with_lists(vec![
        list(1, Currency::EUR, Some(-1000)),
        list(2, Currency::EUR, Some(-500)),
    ]);
    let candidates = explain_price(&ctx, variant(eur(1000)), one()).candidates;
    assert_eq!(candidates[1].result, CandidateResult::Higher);
}

#[test]
fn a_sale_is_measured_against_the_lowest_override_price() {
    let ctx = with_lists(vec![
        list(1, Currency::EUR, Some(-1000)),
        list(2, Currency::EUR, Some(-500)),
        sale(3, Some(-3000)),
    ]);
    let resolved = resolve_price(&ctx, variant(eur(1000)), one()).unwrap();
    assert_eq!(
        (resolved.price, resolved.compare_at),
        (eur(700), Some(eur(900)))
    );
}

#[test]
fn a_sale_above_an_override_price_loses_to_it() {
    let ctx = with_lists(vec![
        list(1, Currency::EUR, Some(-3000)),
        sale(2, Some(-1000)),
    ]);
    let resolved = resolve_price(&ctx, was(eur(1000), eur(1200)), one()).unwrap();
    assert_eq!(
        (resolved.price, resolved.price_list, resolved.compare_at),
        (eur(700), Some(plid(1)), Some(eur(840)))
    );
}

#[test]
fn an_adjustment_for_listed_products_prices_only_them() {
    let ctx = with_lists(vec![scoped(1, -2000, AppliesTo::Products(vec![pid()]))]);
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(800)));
    let other = ProductId::from_uuid(Uuid::from_u128(9));
    let ctx = with_lists(vec![scoped(1, -2000, AppliesTo::Products(vec![other]))]);
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(1000)));
}

#[test]
fn an_adjustment_for_a_category_reaches_its_descendants_products() {
    // The product is in category 2, below category 1: the loader gives both.
    let mut ctx = with_lists(vec![scoped(1, -2000, AppliesTo::Categories(vec![cat(1)]))]);
    ctx.product_categories
        .insert(pid(), HashSet::from([cat(2), cat(1)]));
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(800)));
    ctx.product_categories.clear();
    assert_eq!(
        explain_price(&ctx, variant(eur(1000)), one()).candidates[0].result,
        CandidateResult::NotPricing
    );
}

#[test]
fn a_fixed_price_counts_whatever_the_adjustment_applies_to() {
    let other = ProductId::from_uuid(Uuid::from_u128(9));
    let mut ctx = with_lists(vec![scoped(1, -2000, AppliesTo::Products(vec![other]))]);
    fix(&mut ctx, plid(1), FixedPrice::plain(eur(950)));
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(950)));
}

#[test]
fn an_override_adjusts_the_compare_at_only_where_its_adjustment_reaches() {
    let other = ProductId::from_uuid(Uuid::from_u128(9));
    let mut ctx = with_lists(vec![scoped(1, 1000, AppliesTo::Products(vec![other]))]);
    fix(&mut ctx, plid(1), FixedPrice::plain(eur(900)));
    assert_eq!(
        compare_at_of(&ctx, was(eur(1000), eur(1500))),
        Some(eur(1500))
    );
}

#[test]
fn a_list_not_live_is_skipped_among_others() {
    let mut ended = list(2, Currency::EUR, Some(-5000));
    ended.set_window(hypha_markets::Window::new(None, Some(at(1_000))).unwrap());
    let ctx = with_lists(vec![list(1, Currency::EUR, Some(-1000)), ended]);
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(900)));
    assert_eq!(
        explain_price(&ctx, variant(eur(1000)), one()).candidates[1].result,
        CandidateResult::NotLive(PriceListPhase::Expired)
    );
}

// ---------- quantity prices ----------

fn tiered(price: i64, breaks: &[(u32, i64)]) -> FixedPrice {
    FixedPrice::plain(eur(price))
        .with_breaks(
            breaks
                .iter()
                .map(|(q, p)| hypha_markets::PriceBreak {
                    min_quantity: *q,
                    price: eur(*p),
                })
                .collect(),
        )
        .unwrap()
}

fn qty(n: u32) -> Quantity {
    Quantity::new(n).unwrap()
}

#[test]
fn a_break_prices_from_its_quantity() {
    let mut ctx = with_lists(vec![list(1, Currency::EUR, None)]);
    fix(&mut ctx, plid(1), tiered(1000, &[(10, 800)]));
    let at = |n| resolve_price(&ctx, variant(eur(1200)), qty(n)).map(|r| r.price);
    assert_eq!(at(9), Ok(eur(1000)));
    assert_eq!(at(10), Ok(eur(800)));
}

#[test]
fn each_list_is_priced_at_the_quantity_before_the_lowest_wins() {
    // At 10 the tiered list's break beats the flat sale; at 1 the sale wins.
    let mut ctx = with_lists(vec![
        list(1, Currency::EUR, None),
        list(2, Currency::EUR, Some(-1500)),
    ]);
    fix(&mut ctx, plid(1), tiered(1000, &[(10, 700)]));
    let at = |n| resolve_price(&ctx, variant(eur(1000)), qty(n)).map(|r| (r.price, r.price_list));
    assert_eq!(at(1), Ok((eur(850), Some(plid(2)))));
    assert_eq!(at(10), Ok((eur(700), Some(plid(1)))));
}

#[test]
fn the_winners_breaks_are_shown() {
    let mut ctx = with_lists(vec![list(1, Currency::EUR, None)]);
    let fixed = tiered(1000, &[(5, 900), (10, 800)]);
    fix(&mut ctx, plid(1), fixed.clone());
    assert_eq!(
        resolve_price(&ctx, variant(eur(1000)), one()).map(|r| r.breaks),
        Ok(fixed.breaks().to_vec())
    );
}

#[test]
fn a_sale_break_is_measured_against_the_regular_price_at_the_quantity() {
    let mut ctx = with_lists(vec![list(1, Currency::EUR, Some(-1000)), sale(2, None)]);
    fix(&mut ctx, plid(2), tiered(880, &[(10, 600)]));
    let resolved = resolve_price(&ctx, variant(eur(1000)), qty(10)).unwrap();
    assert_eq!(
        (resolved.price, resolved.compare_at),
        (eur(600), Some(eur(900)))
    );
}

fn group(n: u128) -> CustomerGroupId {
    CustomerGroupId::from_uuid(Uuid::from_u128(300 + n))
}

/// `list(n, …)` for buyers in group `g` only.
fn for_group(n: u128, bp: i32, g: u128) -> PriceList {
    let mut l = list(n, Currency::EUR, Some(bp));
    l.set_audience(Audience::of_groups(vec![group(g)]).unwrap());
    l
}

#[test]
fn a_group_list_prices_nothing_for_a_buyer_in_no_group() {
    let ctx = context(Currency::EUR, Some(for_group(1, -2000, 1)));
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(1000)));
}

#[test]
fn a_group_list_prices_for_a_buyer_in_its_group() {
    let ctx = PricingContext {
        groups: vec![group(2), group(1)],
        ..context(Currency::EUR, Some(for_group(1, -2000, 1)))
    };
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(800)));
}

#[test]
fn a_group_list_prices_nothing_for_a_buyer_in_another_group() {
    let ctx = PricingContext {
        groups: vec![group(2)],
        ..context(Currency::EUR, Some(for_group(1, -2000, 1)))
    };
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(1000)));
}

#[test]
fn a_public_sale_below_a_group_price_still_wins_for_the_group() {
    let ctx = PricingContext {
        groups: vec![group(1)],
        ..with_lists(vec![for_group(1, -1000, 1), sale(2, Some(-3000))])
    };
    assert_eq!(price_of(&ctx, variant(eur(1000))), Ok(eur(700)));
}

#[test]
fn explains_a_list_for_groups_the_buyer_is_not_in() {
    let ctx = context(Currency::EUR, Some(for_group(1, -2000, 1)));
    assert_eq!(
        explain_price(&ctx, variant(eur(1000)), one()).candidates,
        [Candidate {
            price_list: plid(1),
            price: None,
            result: CandidateResult::NotInAudience,
        }]
    );
}
