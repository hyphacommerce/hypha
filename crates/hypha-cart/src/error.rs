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

impl ErrorCode for CartError {
    fn code(&self) -> &'static str {
        match self {
            CartError::ZeroQuantity => "invalid_quantity",
            CartError::QuantityOverflow => "quantity_overflow",
            CartError::LineNotFound(_) => "line_not_found",
        }
    }
}

impl ErrorCode for LineError {
    fn code(&self) -> &'static str {
        match self {
            LineError::ProductNotFound(_) => "product_not_found",
            LineError::ProductNotActive(_) => "product_not_active",
            LineError::VariantNotFound { .. } => "variant_not_in_product",
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
