//! `checkout.validate` v1: every validation app the store has says what's wrong with a
//! cart (errors, which stop checkout) or worth a second look (warnings), each pointing at
//! the part of the cart it's about. Asked when the address is set, when a shipping option
//! is chosen and at checkout; an answer is kept with the cart, so an app is asked again
//! only when what it would be sent changes.

use core::time::Duration;

use hypha_core::{
    AddressVerdict, CartId, ChannelId, CountryCode, Currency, CustomerId, MarketId, MessageCode,
    MessagePath, Money, ProductId, Scope, Severity, ShippingOptionId, ValidationStep, VariantId,
    VerificationFlags,
};
use serde::{Deserialize, Serialize};

use crate::{ExtensionPoint, OnFailure, points::shipping::PostalAddress};

/// At most this many messages per answer.
pub const MAX_VALIDATION_MESSAGES: usize = 20;

/// The longest message an app may write, in characters.
pub const MAX_VALIDATION_TEXT: usize = hypha_core::Message::MAX_TEXT_LEN;

/// Validates a cart for checkout.
pub struct CheckoutValidate;

impl ExtensionPoint for CheckoutValidate {
    const NAME: &'static str = "checkout.validate";
    const VERSION: u16 = 1;
    /// Every validation app is asked at once and checkout waits for the slowest.
    const TIMEOUT: Duration = Duration::from_secs(2);
    /// The merchant decides per app whether its failure blocks checkout
    /// (`block_on_failure`, off by default).
    const ON_FAILURE: OnFailure = OnFailure::Configured;
    /// The app reads the cart; its address and email only with `customer_data:*`.
    const SCOPE: Scope = Scope::CartsRead;
    const READ_ONLY: bool = true;
    type Request = CheckoutValidateRequest;
    type Response = CheckoutValidateResponse;

    /// At most `MAX_VALIDATION_MESSAGES`, each with 1 to `MAX_VALIDATION_TEXT` characters
    /// of text, and a line or group path naming one the request has.
    fn check(
        request: &CheckoutValidateRequest,
        response: &CheckoutValidateResponse,
    ) -> Result<(), String> {
        if response.messages.len() > MAX_VALIDATION_MESSAGES {
            return Err(format!(
                "{} messages, more than {MAX_VALIDATION_MESSAGES}",
                response.messages.len()
            ));
        }
        for (i, m) in response.messages.iter().enumerate() {
            let chars = m.message.trim().chars().count();
            if chars == 0 {
                return Err(format!("message {i} has no text"));
            }
            if chars > MAX_VALIDATION_TEXT {
                return Err(format!(
                    "message {i} is {chars} characters, more than {MAX_VALIDATION_TEXT}"
                ));
            }
            let (index, count) = match m.path {
                MessagePath::Line(n) => (n, request.cart.lines.len()),
                MessagePath::ShippingGroup(n) => (n, request.cart.shipping_groups.len()),
                _ => continue,
            };
            if usize::try_from(index).map_or(true, |n| n >= count) {
                return Err(format!(
                    "message {i} points at {}, which the cart doesn't have",
                    m.path
                ));
            }
        }
        Ok(())
    }
}

/// What Hypha sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CheckoutValidateRequest {
    /// `address` when the shipping address was set, `shipping` when an option was chosen,
    /// `checkout` when the shopper places the order or starts its payment.
    pub step: ValidationStep,
    pub cart: ValidatedCart,
}

/// The cart as the app sees it. Paths in its answer point into the cart's own response,
/// whose arrays are in this order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ValidatedCart {
    pub id: CartId,
    pub market: MarketId,
    pub channel: ChannelId,
    pub country: CountryCode,
    pub currency: Currency,
    pub customer: Option<CustomerId>,
    /// `null` without one, or without the app's `customer_data:email` (then listed in
    /// `redacted`).
    pub email: Option<String>,
    /// Redacted per the app's grant as `shipping.rates`' destination is.
    pub shipping_address: Option<PostalAddress>,
    /// What the store's verification app said about the address, when it was asked.
    pub shipping_address_check: Option<ValidatedAddressCheck>,
    pub lines: Vec<ValidatedLine>,
    /// One per group of shipped lines, with its chosen option.
    pub shipping_groups: Vec<ValidatedShippingGroup>,
    /// The available lines after discounts.
    pub subtotal: Money,
    pub total: Money,
    /// The cart's personal fields left out for this app (`email`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub redacted: Option<Vec<String>>,
}

/// A kept verification, as an app needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ValidatedAddressCheck {
    pub verdict: AddressVerdict,
    pub flags: VerificationFlags,
}

/// One cart line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ValidatedLine {
    pub variant: VariantId,
    pub product: ProductId,
    pub sku: Option<String>,
    pub quantity: u32,
    /// `null` for a line that can't be bought now (sold out, unpublished).
    pub unit_price: Option<Money>,
    /// One unit's weight when the variant is shipped and has one.
    pub weight_grams: Option<u32>,
}

/// A group of shipped lines and the option chosen for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ValidatedShippingGroup {
    /// `null` until the shopper chooses one.
    pub option: Option<ShippingOptionId>,
    /// The option's code (`dhl_express`).
    pub code: Option<String>,
    /// A provider's key for an option a rate app or spore prices (`app_dhl`).
    pub provider: Option<String>,
}

/// The app's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CheckoutValidateResponse {
    /// At most 20; none when the cart is fine.
    pub messages: Vec<ValidationMessage>,
}

/// One message the app has for the shopper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ValidationMessage {
    /// `error` stops checkout; `warning` never does.
    pub severity: Severity,
    /// The app's own: snake_case, a letter first, at most 64 characters.
    #[cfg_attr(feature = "openapi", schema(value_type = String, example = "alcohol_restricted"))]
    pub code: MessageCode,
    /// Where it shows: `$`, `$.email`, `$.shipping_address`, `$.shipping_address.<field>`,
    /// `$.shipping_groups[<i>]` or `$.lines[<i>]`, an index the request has.
    #[cfg_attr(
        feature = "openapi",
        schema(value_type = String, example = "$.shipping_address.postal_code")
    )]
    pub path: MessagePath,
    /// Plain text for the shopper, 1 to 500 characters.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use hypha_core::{AddressField, Currency};
    use uuid::Uuid;

    use super::*;

    fn eur(amount: i64) -> Money {
        Money::new(amount, Currency::EUR).unwrap()
    }

    fn request(lines: usize, groups: usize) -> CheckoutValidateRequest {
        let line = ValidatedLine {
            variant: VariantId::from_uuid(Uuid::from_u128(2)),
            product: ProductId::from_uuid(Uuid::from_u128(3)),
            sku: None,
            quantity: 1,
            unit_price: Some(eur(100)),
            weight_grams: None,
        };
        let group = ValidatedShippingGroup {
            option: None,
            code: None,
            provider: None,
        };
        CheckoutValidateRequest {
            step: ValidationStep::Checkout,
            cart: ValidatedCart {
                id: CartId::from_uuid(Uuid::from_u128(1)),
                market: MarketId::from_uuid(Uuid::from_u128(4)),
                channel: ChannelId::from_uuid(Uuid::from_u128(5)),
                country: CountryCode::new("HR").unwrap(),
                currency: Currency::EUR,
                customer: None,
                email: None,
                shipping_address: None,
                shipping_address_check: None,
                lines: vec![line; lines],
                shipping_groups: vec![group; groups],
                subtotal: eur(100),
                total: eur(100),
                redacted: None,
            },
        }
    }

    fn says(path: MessagePath, text: &str) -> ValidationMessage {
        ValidationMessage {
            severity: Severity::Error,
            code: MessageCode::new("alcohol_restricted").unwrap(),
            path,
            message: text.to_string(),
        }
    }

    fn answer(messages: Vec<ValidationMessage>) -> CheckoutValidateResponse {
        CheckoutValidateResponse { messages }
    }

    #[test]
    fn no_messages_is_an_answer() {
        assert_eq!(
            CheckoutValidate::check(&request(1, 1), &answer(vec![])),
            Ok(())
        );
    }

    #[test]
    fn twenty_messages_are_taken() {
        let m = says(MessagePath::Cart, "No.");
        assert_eq!(
            CheckoutValidate::check(&request(1, 1), &answer(vec![m; 20])),
            Ok(())
        );
    }

    #[test]
    fn twenty_one_messages_are_refused() {
        let m = says(MessagePath::Cart, "No.");
        assert!(CheckoutValidate::check(&request(1, 1), &answer(vec![m; 21])).is_err());
    }

    #[test]
    fn blank_text_is_refused() {
        let m = says(MessagePath::Cart, "  ");
        assert!(CheckoutValidate::check(&request(1, 1), &answer(vec![m])).is_err());
    }

    #[test]
    fn text_of_500_characters_is_taken() {
        let m = says(MessagePath::Cart, &"a".repeat(500));
        assert_eq!(
            CheckoutValidate::check(&request(1, 1), &answer(vec![m])),
            Ok(())
        );
    }

    #[test]
    fn text_of_501_characters_is_refused() {
        let m = says(MessagePath::Cart, &"a".repeat(501));
        assert!(CheckoutValidate::check(&request(1, 1), &answer(vec![m])).is_err());
    }

    #[test]
    fn a_line_the_cart_has_is_taken() {
        let m = says(MessagePath::Line(1), "No.");
        assert_eq!(
            CheckoutValidate::check(&request(2, 1), &answer(vec![m])),
            Ok(())
        );
    }

    #[test]
    fn a_line_past_the_carts_is_refused() {
        let m = says(MessagePath::Line(2), "No.");
        assert!(CheckoutValidate::check(&request(2, 1), &answer(vec![m])).is_err());
    }

    #[test]
    fn a_group_past_the_carts_is_refused() {
        let m = says(MessagePath::ShippingGroup(0), "No.");
        assert!(CheckoutValidate::check(&request(1, 0), &answer(vec![m])).is_err());
    }

    #[test]
    fn an_address_field_is_taken_without_an_address() {
        let m = says(
            MessagePath::ShippingAddressField(AddressField::PostalCode),
            "No.",
        );
        assert_eq!(
            CheckoutValidate::check(&request(1, 1), &answer(vec![m])),
            Ok(())
        );
    }

    #[test]
    fn a_path_outside_the_set_doesnt_parse() {
        let json =
            r#"{"messages":[{"severity":"error","code":"x","path":"$.lines[*]","message":"No."}]}"#;
        assert!(serde_json::from_str::<CheckoutValidateResponse>(json).is_err());
    }

    #[test]
    fn the_request_is_sent_in_the_wire_shape() {
        let json = serde_json::to_value(request(1, 1)).unwrap();
        assert_eq!(json["step"], "checkout");
        assert_eq!(json["cart"]["lines"][0]["quantity"], 1);
        assert!(json["cart"].get("redacted").is_none());
    }
}
