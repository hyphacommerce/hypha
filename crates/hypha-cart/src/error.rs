use hypha_core::{ErrorCode, MoneyError, ProductId, VariantId};
use hypha_pricing::PricingError;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CartError {
    #[error("quantity must be at least 1")]
    ZeroQuantity,
    #[error("quantity is too large")]
    QuantityOverflow,
    #[error("variant {0} is not in the cart")]
    LineNotFound(VariantId),
    #[error("a cart linked to a customer needs an email; unlink the customer first")]
    EmailRequired,
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
    /// The market's locations can't supply the line's quantity (design note 0004, I6).
    #[error("variant {variant}: only {available} of {requested} in stock")]
    OutOfStock {
        variant: VariantId,
        requested: u32,
        available: i64,
    },
    #[error(transparent)]
    Pricing(#[from] PricingError),
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// Why stored data can't be rebuilt into a cart. Only `Cart::restore` returns it and
/// adapters report it as a backend failure, so it has no API code (review plan D4).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RestoreError {
    #[error(transparent)]
    Cart(#[from] CartError),
    #[error("variant {0} is on more than one line")]
    DuplicateLine(VariantId),
}

impl ErrorCode for CartError {
    fn code(&self) -> &'static str {
        match self {
            CartError::ZeroQuantity => "invalid_quantity",
            CartError::QuantityOverflow => "quantity_overflow",
            CartError::LineNotFound(_) => "line_not_found",
            CartError::EmailRequired => "email_required",
        }
    }
}

impl ErrorCode for LineError {
    fn code(&self) -> &'static str {
        match self {
            LineError::ProductNotFound(_) => "product_not_found",
            LineError::ProductNotActive(_) => "product_not_active",
            LineError::VariantNotFound { .. } => "variant_not_in_product",
            LineError::OutOfStock { .. } => "out_of_stock",
            LineError::Pricing(e) => e.code(),
            LineError::Money(e) => e.code(),
        }
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{MarketId, ProductId};
    use hypha_pricing::PricingError;
    use uuid::Uuid;

    use super::*;
    use crate::CommandError;

    #[test]
    fn wrapped_errors_report_the_innermost_code() {
        let not_available = PricingError::NotAvailable {
            product: ProductId::from_uuid(Uuid::from_u128(1)),
            market: MarketId::from_uuid(Uuid::from_u128(2)),
        };
        let e = CommandError::Line(LineError::Pricing(not_available));
        assert_eq!(e.code(), "not_available_in_market");
    }

    #[test]
    fn own_variants_have_their_own_codes() {
        assert_eq!(CartError::ZeroQuantity.code(), "invalid_quantity");
        assert_eq!(
            LineError::ProductNotActive(ProductId::from_uuid(Uuid::from_u128(1))).code(),
            "product_not_active"
        );
    }
}
