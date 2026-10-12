use hypha_core::{Currency, MarketId, MoneyError, PriceListId, ProductId};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PricingError {
    #[error("product {product} is not available in market {market}")]
    NotAvailable {
        product: ProductId,
        market: MarketId,
    },
    #[error("price list {0} is referenced by a catalog but was not provided")]
    PriceListNotFound(PriceListId),
    /// Until exchange rates exist, a price can't be converted to the market's currency.
    #[error("price is in {found:?}, but the market sells in {expected:?}")]
    CurrencyMismatch { expected: Currency, found: Currency },
    #[error(transparent)]
    Money(#[from] MoneyError),
}
