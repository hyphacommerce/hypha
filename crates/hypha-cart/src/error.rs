use hypha_core::{MoneyError, ProductId, VariantId};
use hypha_pricing::PricingError;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CartError {
    #[error("quantity must be at least 1")]
    ZeroQuantity,
    #[error("quantity is too large")]
    QuantityOverflow,
    #[error("variant {0} is not in the cart")]
    LineNotFound(VariantId),
}

/// Why a line can't be priced. Returned as an error by `price_line` (and so by `add_line`),
/// and kept as the reason in `LineStatus::Unavailable` when a whole cart is priced.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum LineError {
    #[error("product {0} not found")]
    ProductNotFound(ProductId),
    #[error("product {0} is not active")]
    ProductNotActive(ProductId),
    #[error("variant {variant} does not belong to product {product}")]
    VariantNotFound {
        product: ProductId,
        variant: VariantId,
    },
    #[error(transparent)]
    Pricing(#[from] PricingError),
    #[error(transparent)]
    Money(#[from] MoneyError),
}
