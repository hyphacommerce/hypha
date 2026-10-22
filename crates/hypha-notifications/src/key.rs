//! Which emails exist: a closed set, because each one has a trigger and a context (its
//! variables) that only code can provide.

use std::{fmt, str::FromStr};

/// One of the store's templates. The names are contracts: the API, the stored rows and
/// the default files (`defaults/<key>.*.j2`) use them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TemplateKey {
    /// The frame every other template extends (`{% extends "layout" %}`): branding once.
    Layout,
    OrderConfirmation,
    OrderShipped,
    OrderTrackingUpdated,
    OrderCancelled,
    StaffNewOrder,
    StaffInvite,
    StaffPasswordReset,
    /// A customer's sign-in code.
    CustomerLoginCode,
    /// A customer asked from their account for their data to be erased: when it runs, and
    /// that they can still cancel.
    CustomerErasureRequested,
}

/// Who a template's messages go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Audience {
    /// The order's email address.
    Buyer,
    /// Staff: the settings' recipients, an invitee, a staff member resetting a password.
    Staff,
}

impl TemplateKey {
    /// Every key: the layout first, then the buyer's templates, then the staff's, the
    /// order the admin lists them in.
    pub const ALL: [TemplateKey; 10] = [
        TemplateKey::Layout,
        TemplateKey::OrderConfirmation,
        TemplateKey::OrderShipped,
        TemplateKey::OrderTrackingUpdated,
        TemplateKey::OrderCancelled,
        TemplateKey::CustomerLoginCode,
        TemplateKey::CustomerErasureRequested,
        TemplateKey::StaffNewOrder,
        TemplateKey::StaffInvite,
        TemplateKey::StaffPasswordReset,
    ];

    /// The same string used in JSON, storage and the default files' names.
    pub fn as_str(self) -> &'static str {
        match self {
            TemplateKey::Layout => "layout",
            TemplateKey::OrderConfirmation => "order_confirmation",
            TemplateKey::OrderShipped => "order_shipped",
            TemplateKey::OrderTrackingUpdated => "order_tracking_updated",
            TemplateKey::OrderCancelled => "order_cancelled",
            TemplateKey::StaffNewOrder => "staff_new_order",
            TemplateKey::StaffInvite => "staff_invite",
            TemplateKey::StaffPasswordReset => "staff_password_reset",
            TemplateKey::CustomerLoginCode => "customer_login_code",
            TemplateKey::CustomerErasureRequested => "customer_erasure_requested",
        }
    }

    /// Whether a merchant may turn it off. The layout isn't a message; invites and
    /// password resets are how staff get in, and the sign-in code how customers do:
    /// without them nobody could join, recover an account or sign in.
    pub fn can_disable(self) -> bool {
        match self {
            TemplateKey::OrderConfirmation
            | TemplateKey::OrderShipped
            | TemplateKey::OrderTrackingUpdated
            | TemplateKey::OrderCancelled
            | TemplateKey::CustomerErasureRequested
            | TemplateKey::StaffNewOrder => true,
            TemplateKey::Layout
            | TemplateKey::StaffInvite
            | TemplateKey::StaffPasswordReset
            | TemplateKey::CustomerLoginCode => false,
        }
    }

    /// Who its messages go to; `None` for the layout, which is never sent on its own.
    pub fn audience(self) -> Option<Audience> {
        match self {
            TemplateKey::Layout => None,
            TemplateKey::OrderConfirmation
            | TemplateKey::OrderShipped
            | TemplateKey::OrderTrackingUpdated
            | TemplateKey::OrderCancelled
            | TemplateKey::CustomerLoginCode
            | TemplateKey::CustomerErasureRequested => Some(Audience::Buyer),
            TemplateKey::StaffNewOrder
            | TemplateKey::StaffInvite
            | TemplateKey::StaffPasswordReset => Some(Audience::Staff),
        }
    }

    /// Whether it's the shared frame rather than a message: it has an HTML body only.
    pub fn is_layout(self) -> bool {
        self == TemplateKey::Layout
    }
}

impl fmt::Display for TemplateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TemplateKey {
    /// The text that didn't name a template.
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        TemplateKey::ALL
            .into_iter()
            .find(|key| key.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_parses_back_from_its_string() {
        for key in TemplateKey::ALL {
            assert_eq!(key.as_str().parse(), Ok(key));
            assert_eq!(key.to_string(), key.as_str());
        }
        assert_eq!(
            "Order_Confirmation".parse::<TemplateKey>(),
            Err("Order_Confirmation".to_string())
        );
    }

    #[test]
    fn only_order_new_order_and_erasure_emails_can_be_disabled() {
        let disabled: Vec<TemplateKey> = TemplateKey::ALL
            .into_iter()
            .filter(|k| k.can_disable())
            .collect();
        assert_eq!(
            disabled,
            [
                TemplateKey::OrderConfirmation,
                TemplateKey::OrderShipped,
                TemplateKey::OrderTrackingUpdated,
                TemplateKey::OrderCancelled,
                TemplateKey::CustomerErasureRequested,
                TemplateKey::StaffNewOrder,
            ]
        );
    }

    #[test]
    fn the_layout_has_no_audience() {
        assert_eq!(TemplateKey::Layout.audience(), None);
        assert_eq!(TemplateKey::OrderShipped.audience(), Some(Audience::Buyer));
        assert_eq!(TemplateKey::StaffInvite.audience(), Some(Audience::Staff));
    }
}
