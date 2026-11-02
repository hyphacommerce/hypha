//! A cart's messages: what's wrong with it (errors, which stop checkout and starting its
//! payment) and what's worth a second look (warnings, which never do), composed from every
//! source in one order, so the cart's responses and checkout agree.

use std::collections::HashMap;

use hypha_cart::{PricedCart, PricingPorts, product_categories};
use hypha_checkout_rules::{
    CheckoutRuleRepo, HasCheckoutRules, RuleInput, RuleLine, VerifiedFacts, evaluate,
    names_categories,
};
use hypha_core::{
    AddressFlags, AddressFormats, CartId, Message, MessagePath, MessageSource, ProductId, RepoError,
};

use crate::{
    CommandError,
    validation::{CheckoutValidators, NoValidators, app_messages},
};

/// What a cart's messages come from beside the cart and the store's own data: the host's
/// address formats and its validation apps. One parameter for commands to take, so a
/// source joining it doesn't change their signatures.
pub trait CartChecks {
    type Formats: AddressFormats;
    type Validators: CheckoutValidators;

    fn formats(&self) -> &Self::Formats;

    fn validators(&self) -> &Self::Validators;
}

/// Address formats alone: no validation apps are asked.
impl<F: AddressFormats> CartChecks for F {
    type Formats = F;
    type Validators = NoValidators;

    fn formats(&self) -> &F {
        self
    }

    fn validators(&self) -> &NoValidators {
        &NoValidators
    }
}

/// The address formats and the validation apps a host gives.
#[derive(Debug, Clone, Copy)]
pub struct Checks<F, V> {
    pub formats: F,
    pub validators: V,
}

impl<F: AddressFormats, V: CheckoutValidators> CartChecks for Checks<F, V> {
    type Formats = F;
    type Validators = V;

    fn formats(&self) -> &F {
        &self.formats
    }

    fn validators(&self) -> &V {
        &self.validators
    }
}

/// The cart's messages, in the order a storefront shows them: the address's format
/// warnings, what its verification found (kept with the cart, never asked again here),
/// the merchant's checkout rules in their order, then what each validation app last said
/// (kept with the cart: nobody is asked here). A checked-out cart has none: nothing about
/// it can change.
///
/// The address isn't checked again for its problems: it passed them when it was set, and
/// an address stored before a format changed still reads.
pub async fn cart_messages<S: PricingPorts + HasCheckoutRules>(
    ports: &S,
    checks: &impl CartChecks,
    cart: &PricedCart,
) -> Result<Vec<Message>, RepoError> {
    if cart.order.is_some() {
        return Ok(Vec::new());
    }
    let mut messages = Vec::new();
    let checked = cart
        .shipping_address
        .as_ref()
        .map(|address| checks.formats().check(address));
    if let Some(checked) = &checked {
        messages.extend(checked.warnings.iter().map(Message::format_warning));
    }
    let verification = cart
        .address_check
        .as_ref()
        .filter(|_| cart.shipping_address.is_some());
    messages.extend(verification.and_then(Message::verification_warning));

    let rules = ports.checkout_rules().list().await?;
    if rules.iter().any(|r| r.enabled()) {
        let categories = if names_categories(&rules) {
            let products: Vec<ProductId> = cart.lines.iter().map(|l| l.product).collect();
            product_categories(ports, &products).await?
        } else {
            HashMap::new()
        };
        let input = RuleInput {
            address: checked.as_ref().map(|c| c.address.parts().clone()),
            flags: checked
                .as_ref()
                .map_or(AddressFlags::default(), |c| c.flags),
            verification: verification.map(|v| VerifiedFacts {
                verdict: Some(v.verdict),
                po_box: v.flags.po_box,
                residential: v.flags.residential,
                business: v.flags.business,
                vacant: v.flags.vacant,
            }),
            email: cart.email.as_ref().map(|e| e.as_str().to_string()),
            country: cart.country,
            market: Some(cart.market),
            channel: Some(cart.channel),
            shipping: cart
                .shipping_groups
                .iter()
                .map(|g| g.selected.as_ref().map(|s| s.option))
                .collect(),
            lines: cart
                .lines
                .iter()
                .map(|l| RuleLine {
                    product: l.product,
                    categories: categories.get(&l.product).cloned().unwrap_or_default(),
                    quantity: l.quantity.value(),
                    shipping_weight: l.details.as_ref().and_then(|d| d.shipping_weight),
                })
                .collect(),
            subtotal: Some(cart.subtotal),
        };
        messages.extend(evaluate(&input, &rules));
    }
    let validators = checks.validators().validators(cart).await?;
    messages.extend(app_messages(&validators, cart));
    Ok(without_echoed_warnings(messages))
}

/// `messages` without the built-in warnings (the format's, the verification's) on a path
/// that already carries an error: the shopper has that error to fix there, and a
/// merchant's rule often says the same thing more firmly (a house number required where
/// the format only suspects it's missing).
fn without_echoed_warnings(messages: Vec<Message>) -> Vec<Message> {
    let errors: Vec<MessagePath> = messages
        .iter()
        .filter(|m| m.is_error())
        .map(Message::path)
        .collect();
    messages
        .into_iter()
        .filter(|m| {
            let built_in = matches!(
                m.source(),
                MessageSource::Format | MessageSource::Verification
            );
            m.is_error() || !built_in || !errors.contains(&m.path())
        })
        .collect()
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
    use hypha_core::{AddressField, AddressWarning, AddressWarningCode, MessageCode, Severity};
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

    fn rule_on_line1(severity: Severity) -> Message {
        Message::new(
            severity,
            MessageCode::new("house_number_required").unwrap(),
            MessagePath::ShippingAddressField(AddressField::Line1),
            "Add your house number.",
            MessageSource::Rule {
                rule: hypha_core::CheckoutRuleId::from_uuid(Uuid::from_u128(9)),
            },
        )
        .unwrap()
    }

    #[test]
    fn a_built_in_warning_under_an_error_is_left_out() {
        assert_eq!(
            without_echoed_warnings(vec![warning(), rule_on_line1(Severity::Error)]),
            vec![rule_on_line1(Severity::Error)]
        );
    }

    #[test]
    fn a_built_in_warning_beside_another_warning_stays() {
        assert_eq!(
            without_echoed_warnings(vec![warning(), rule_on_line1(Severity::Warning)]),
            vec![warning(), rule_on_line1(Severity::Warning)]
        );
    }

    #[test]
    fn a_built_in_warning_elsewhere_than_the_error_stays() {
        assert_eq!(
            without_echoed_warnings(vec![warning(), error()]),
            vec![warning(), error()]
        );
    }
}
