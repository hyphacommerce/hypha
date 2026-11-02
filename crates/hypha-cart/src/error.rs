use hypha_core::{
    AddressError, CountryCode, ErrorCode, MoneyError, OrderId, ProductId, QuantityError, VariantId,
};
use hypha_pricing::PricingError;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CartError {
    #[error(transparent)]
    Quantity(#[from] QuantityError),
    #[error("variant {0} is not in the cart")]
    LineNotFound(VariantId),
    #[error("a cart linked to a customer needs an email; unlink the customer first")]
    EmailRequired,
    /// A signed-in customer's cart is for them: the order is theirs, so the email stays
    /// theirs.
    #[error("the cart is a customer's: its email is theirs")]
    CustomerEmailFixed,
    /// Checked out: the cart became `order` and can't change any more.
    #[error("the cart was checked out as order {order}")]
    Completed { order: OrderId },
    /// The cart's country picked its market and taxes; an address elsewhere would need
    /// another cart.
    #[error("the address is in {address}, but the cart ships to {cart}")]
    AddressCountryMismatch {
        address: CountryCode,
        cart: CountryCode,
    },
    #[error("set a shipping address first")]
    ShippingAddressRequired,
    #[error("a cart holds at most {max} discount codes")]
    TooManyDiscountCodes { max: usize },
    #[error("the cart doesn't hold the code {0:?}")]
    DiscountCodeNotOnCart(String),
}

/// Why a line can't be priced. Returned as an error by `price_line` (and so by `add_line`),
/// and kept as the reason in `LineStatus::Unavailable` when a whole cart is priced.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum LineError {
    #[error("product {0} not found")]
    ProductNotFound(ProductId),
    #[error("product {0} is not active")]
    ProductNotActive(ProductId),
    /// Not sold through the cart's channel.
    #[error("product {0} isn't sold in this channel")]
    ProductNotPublished(ProductId),
    #[error("variant {variant} does not belong to product {product}")]
    VariantNotFound {
        product: ProductId,
        variant: VariantId,
    },
    /// The market's locations can't supply the line's quantity.
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
/// adapters report it as a backend failure, so it has no API code.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum RestoreError {
    #[error(transparent)]
    Cart(#[from] CartError),
    #[error("variant {0} is on more than one line")]
    DuplicateLine(VariantId),
    #[error(transparent)]
    Address(#[from] AddressError),
    #[error("the discount code {0:?} is held twice")]
    DuplicateDiscountCode(String),
}

impl ErrorCode for CartError {
    fn code(&self) -> &'static str {
        match self {
            CartError::Quantity(e) => e.code(),
            CartError::LineNotFound(_) => "line_not_found",
            CartError::EmailRequired => "email_required",
            CartError::CustomerEmailFixed => "cart_email_fixed",
            CartError::Completed { .. } => "cart_completed",
            CartError::AddressCountryMismatch { .. } => "address_country_mismatch",
            CartError::ShippingAddressRequired => "shipping_address_required",
            CartError::TooManyDiscountCodes { .. } => "too_many_discount_codes",
            CartError::DiscountCodeNotOnCart(_) => "discount_code_not_on_cart",
        }
    }
}

impl ErrorCode for LineError {
    fn code(&self) -> &'static str {
        match self {
            LineError::ProductNotFound(_) => "product_not_found",
            LineError::ProductNotActive(_) => "product_not_active",
            LineError::ProductNotPublished(_) => "product_not_published",
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
        assert_eq!(
            CartError::Quantity(QuantityError::Zero).code(),
            "invalid_quantity"
        );
        assert_eq!(
            LineError::ProductNotActive(ProductId::from_uuid(Uuid::from_u128(1))).code(),
            "product_not_active"
        );
    }
}
