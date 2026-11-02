//! A cart's messages: what's wrong with it (errors, which stop checkout and starting its
//! payment) and what's worth a second look (warnings, which never do), composed from every
//! source in one order, so the cart's responses and checkout agree.

use hypha_cart::PricedCart;
use hypha_core::{AddressFormats, CartId, Message};

use crate::CommandError;

/// What a cart's messages come from beside the cart itself: the host's address formats.
/// One parameter for commands to take, so a source joining it doesn't change their
/// signatures.
pub trait CartChecks {
    type Formats: AddressFormats;

    fn formats(&self) -> &Self::Formats;
}

/// Address formats alone are enough while nothing else is asked.
impl<F: AddressFormats> CartChecks for F {
    type Formats = F;

    fn formats(&self) -> &F {
        self
    }
}

/// The cart's messages, in the order a storefront shows them: the address's format
/// warnings first. A checked-out cart has none: nothing about it can change.
///
/// The address isn't checked again for its problems: it passed them when it was set, and
/// an address stored before a format changed still reads.
pub fn cart_messages(checks: &impl CartChecks, cart: &PricedCart) -> Vec<Message> {
    if cart.order.is_some() {
        return Vec::new();
    }
    let mut messages = Vec::new();
    if let Some(address) = &cart.shipping_address {
        let checked = checks.formats().check(address);
        messages.extend(checked.warnings.iter().map(Message::format_warning));
    }
    messages
}

/// `checkout_blocked` with every error among `messages`, warnings left out; nothing when
/// there's none.
pub(crate) fn refuse_errors(cart: CartId, messages: Vec<Message>) -> Result<(), CommandError> {
    let errors: Vec<Message> = messages.into_iter().filter(Message::is_error).collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CommandError::CheckoutBlocked {
            cart,
            messages: errors,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hypha_core::{
        AddressField, AddressWarning, AddressWarningCode, MessageCode, MessagePath, MessageSource,
        Severity,
    };
    use uuid::Uuid;

    fn cart() -> CartId {
        CartId::from_uuid(Uuid::from_u128(1))
    }

    fn warning() -> Message {
        Message::format_warning(&AddressWarning {
            field: AddressField::Line1,
            code: AddressWarningCode::HouseNumberMissing,
            message: "Add a house number".to_string(),
        })
    }

    fn error() -> Message {
        Message::new(
            Severity::Error,
            MessageCode::new("po_box_not_allowed").unwrap(),
            MessagePath::ShippingGroup(0),
            "No PO boxes with this option.",
            MessageSource::Format,
        )
        .unwrap()
    }

    #[test]
    fn no_messages_let_checkout_go_on() {
        assert_eq!(refuse_errors(cart(), Vec::new()), Ok(()));
    }

    #[test]
    fn warnings_alone_let_checkout_go_on() {
        assert_eq!(refuse_errors(cart(), vec![warning()]), Ok(()));
    }

    #[test]
    fn an_error_blocks_checkout_with_the_errors_alone() {
        assert_eq!(
            refuse_errors(cart(), vec![warning(), error(), warning()]),
            Err(CommandError::CheckoutBlocked {
                cart: cart(),
                messages: vec![error()],
            })
        );
    }
}
