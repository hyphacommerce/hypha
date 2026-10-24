//! Shipping on a priced cart: the group of lines that need shipping, and what the buyer's
//! choice costs now. Offers and selection are commands (`commands.rs`); this is the read
//! side, with no I/O.

use hypha_core::{
    Money, MoneyError, ProductId, Quantity, ShippingGroupId, ShippingOptionId, TotalWeight,
    VariantId,
};
use hypha_discounts::Share;
use hypha_products::Product;
use hypha_shipping::{DeliveryEstimate, OptionRate, ShippingZone, Unpriced};
use hypha_tax::{TaxContext, TaxLine, TaxableKind, TaxableLine, tax_line};

use crate::{Cart, SelectedShipping, StoredQuote};

/// What pricing needs to price a cart's shipping: the zone of its country, if any.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShippingContext {
    pub zone: Option<ShippingZone>,
}

/// A group of lines shipped together, with the buyer's choice (UCP's `groups[]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedShippingGroup {
    pub id: ShippingGroupId,
    /// The available lines that need shipping.
    pub lines: Vec<VariantId>,
    pub selected: Option<PricedShipping>,
    /// Why the chosen option can't be charged now; the buyer picks again.
    pub problem: Option<ShippingProblem>,
}

/// The chosen option at its price now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedShipping {
    pub option: ShippingOptionId,
    pub code: String,
    pub name: String,
    /// The option's price, before shipping discounts.
    pub amount: Money,
    /// What shipping discounts took off it.
    pub discounts: Vec<Share>,
    /// What the buyer pays for it: `amount` less the discounts. Tax is on this.
    pub total: Money,
    /// `None` where the cart's country has no tax region.
    pub tax: Option<TaxLine>,
    pub estimate: Option<DeliveryEstimate>,
    /// For a provider's option: its quote, which checkout renews once expired.
    pub quote: Option<StoredQuote>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShippingProblem {
    /// The option or its zone is gone, disabled, or no longer the one quoted.
    OptionUnavailable(ShippingOptionId),
    /// No price in the cart's currency.
    NoPriceInCurrency(ShippingOptionId),
    /// Weight tiers, and a line's weight isn't known.
    WeightUnknown(ShippingOptionId),
    /// Adding shipping would push a total past `Money::MAX_AMOUNT`.
    Money(MoneyError),
}

impl ShippingProblem {
    pub fn code(&self) -> &'static str {
        match self {
            ShippingProblem::OptionUnavailable(_) => "shipping_option_unavailable",
            ShippingProblem::NoPriceInCurrency(_) => "no_price_in_currency",
            ShippingProblem::WeightUnknown(_) => "weight_unknown",
            ShippingProblem::Money(e) => hypha_core::ErrorCode::code(e),
        }
    }
}

/// The lines of the cart's shipping group and their weight, before anything is priced:
/// `None` when no available line needs shipping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GroupBasis {
    pub lines: Vec<VariantId>,
    /// `None` when a line's weight isn't known.
    pub weight: Option<TotalWeight>,
}

/// The group of `available` lines (product, variant, quantity) that need shipping.
pub(crate) fn group_basis(
    available: impl Iterator<Item = (ProductId, VariantId, Quantity)>,
    products: &[Product],
) -> Option<GroupBasis> {
    let mut lines = Vec::new();
    let mut weight = Some(TotalWeight::default());
    for (product, variant, quantity) in available {
        let found = products
            .iter()
            .find(|p| p.id() == product)
            .and_then(|p| p.variant(variant));
        let Some(found) = found.filter(|v| v.requires_shipping()) else {
            continue;
        };
        lines.push(variant);
        weight = weight.and_then(|w| Some(w.add(found.weight()?, quantity.value())));
    }
    (!lines.is_empty()).then_some(GroupBasis { lines, weight })
}

/// What the buyer's choice costs for a group whose lines come to `subtotal`, before
/// shipping discounts: what discounts weigh a rate by.
pub(crate) fn rate_for(
    cart: &Cart,
    basis: &GroupBasis,
    subtotal: Money,
    tax: &TaxContext,
    shipping: &ShippingContext,
) -> Option<Money> {
    let choice = cart.shipping()?;
    price_choice(choice, subtotal, basis.weight, &[], tax, shipping)
        .ok()
        .map(|p| p.amount)
}

/// The cart's shipping group, priced for its lines' `subtotal` (after discounts), with the
/// shipping discounts' `shares`. Its price isn't in any total yet;
/// `price_cart_with_shipping` adds it.
pub(crate) fn price_group(
    cart: &Cart,
    basis: GroupBasis,
    subtotal: Money,
    shares: &[Share],
    tax: &TaxContext,
    shipping: &ShippingContext,
) -> PricedShippingGroup {
    let (selected, problem) = match cart.shipping() {
        None => (None, None),
        Some(choice) => match price_choice(choice, subtotal, basis.weight, shares, tax, shipping) {
            Ok(priced) => (Some(priced), None),
            Err(problem) => (None, Some(problem)),
        },
    };
    PricedShippingGroup {
        id: cart.shipping_group(),
        lines: basis.lines,
        selected,
        problem,
    }
}

/// The buyer's choice at its price now: a built-in rate recomputed for the group, a
/// provider's as quoted (if the option still asks that provider for that service), less
/// the shipping discounts' `shares`.
fn price_choice(
    choice: &SelectedShipping,
    subtotal: Money,
    weight: Option<TotalWeight>,
    shares: &[Share],
    tax: &TaxContext,
    shipping: &ShippingContext,
) -> Result<PricedShipping, ShippingProblem> {
    let option = shipping
        .zone
        .as_ref()
        .and_then(|z| z.option(choice.option))
        .filter(|o| o.enabled())
        .ok_or(ShippingProblem::OptionUnavailable(choice.option))?;
    let (amount, quote) = match (option.rate(), &choice.quote) {
        (OptionRate::Builtin(rate), _) => match rate.price(subtotal, weight) {
            Ok(price) => (price.money(), None),
            Err(Unpriced::NoPriceInCurrency) => {
                return Err(ShippingProblem::NoPriceInCurrency(option.id()));
            }
            Err(Unpriced::WeightUnknown) => {
                return Err(ShippingProblem::WeightUnknown(option.id()));
            }
        },
        (OptionRate::Provider { provider, service }, Some(quote))
            if *provider == quote.provider
                && *service == quote.service
                && quote.amount.money().currency() == subtotal.currency() =>
        {
            (quote.amount.money(), Some(quote.clone()))
        }
        // Another provider or service than quoted, or no quote: not priceable offline.
        (OptionRate::Provider { .. }, _) => {
            return Err(ShippingProblem::OptionUnavailable(option.id()));
        }
    };
    let mut total = amount;
    for share in shares {
        total = total
            .checked_sub(share.amount)
            .map_err(ShippingProblem::Money)?;
    }
    let taxable = TaxableLine {
        kind: TaxableKind::Shipping {
            class: option.tax_class(),
        },
        amount: total,
    };
    let tax = tax_line(tax, &taxable).map_err(ShippingProblem::Money)?;
    Ok(PricedShipping {
        option: option.id(),
        code: option.code().as_str().to_string(),
        name: option.name().to_string(),
        amount,
        discounts: shares.to_vec(),
        total,
        tax,
        estimate: option.estimate(),
        quote,
    })
}
