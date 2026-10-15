//! Shipping on a priced cart (design note 0015, sections 6 and 7): the group of lines
//! that need shipping, and what the buyer's choice costs now. Offers and selection are
//! commands (`commands.rs`); this is the read side, with no I/O.

use hypha_core::{
    Currency, Money, MoneyError, ShippingGroupId, ShippingOptionId, TotalWeight, VariantId,
};
use hypha_products::Product;
use hypha_shipping::{DeliveryEstimate, OptionRate, ShippingZone, Unpriced};
use hypha_tax::{TaxContext, TaxLine, TaxableKind, TaxableLine, tax_line};

use crate::{Cart, LineStatus, PricedLine, SelectedShipping, StoredQuote};

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
    pub amount: Money,
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

/// The cart's shipping group, priced: `None` when no available line needs shipping.
/// Its price isn't in any total yet; `price_cart_with_shipping` adds it.
pub(crate) fn price_group(
    cart: &Cart,
    currency: Currency,
    lines: &[PricedLine],
    products: &[Product],
    tax: &TaxContext,
    shipping: &ShippingContext,
) -> Option<PricedShippingGroup> {
    let mut group_lines = Vec::new();
    let mut subtotal = Money::zero(currency);
    let mut weight = Some(TotalWeight::default());
    for line in lines {
        let LineStatus::Priced { total, .. } = &line.status else {
            continue;
        };
        let variant = products
            .iter()
            .find(|p| p.id() == line.product)
            .and_then(|p| p.variant(line.variant));
        let Some(variant) = variant.filter(|v| v.requires_shipping()) else {
            continue;
        };
        group_lines.push(line.variant);
        // Priced lines already fit the cart's totals together, so their sum fits too.
        subtotal = subtotal.checked_add(*total).unwrap_or(subtotal);
        weight = weight.and_then(|w| Some(w.add(variant.weight()?, line.quantity.value())));
    }
    if group_lines.is_empty() {
        return None;
    }

    let (selected, problem) = match cart.shipping() {
        None => (None, None),
        Some(choice) => match price_choice(choice, subtotal, weight, tax, shipping) {
            Ok(priced) => (Some(priced), None),
            Err(problem) => (None, Some(problem)),
        },
    };
    Some(PricedShippingGroup {
        id: cart.shipping_group(),
        lines: group_lines,
        selected,
        problem,
    })
}

/// The buyer's choice at its price now: a built-in rate recomputed for the group, a
/// provider's as quoted (if the option still asks that provider for that service).
fn price_choice(
    choice: &SelectedShipping,
    subtotal: Money,
    weight: Option<TotalWeight>,
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
    let taxable = TaxableLine {
        kind: TaxableKind::Shipping {
            class: option.tax_class(),
        },
        amount,
    };
    let tax = tax_line(tax, &taxable).map_err(ShippingProblem::Money)?;
    Ok(PricedShipping {
        option: option.id(),
        code: option.code().as_str().to_string(),
        name: option.name().to_string(),
        amount,
        tax,
        estimate: option.estimate(),
        quote,
    })
}
