//! `admin.action` v1: a staff member pressed one of the app's admin actions (a button on an
//! order, a payment, a product), and Hypha calls the action's URL from the app's manifest.
//! The app acts through its own token; the answer only tells the admin what to show next.

use core::time::Duration;

use hypha_core::{CartId, CustomerId, FulfillmentId, OrderId, PaymentId, ProductId, Scope, UserId};
use serde::{Deserialize, Serialize};

use crate::{ExtensionPoint, OnFailure};

/// Runs an app's admin action.
pub struct RunAdminAction;

/// The longest `message` an answer may carry, as the bridge's toasts take.
pub const MAX_ACTION_MESSAGE: usize = 500;

impl ExtensionPoint for RunAdminAction {
    const NAME: &'static str = "admin.action";
    const VERSION: u16 = 1;
    const TIMEOUT: Duration = Duration::from_secs(10);
    /// The admin shows `app_unavailable` (502); nothing in Hypha changed.
    const ON_FAILURE: OnFailure = OnFailure::FailClosed;
    /// Not a registration (`KNOWN` doesn't list it: actions come from the manifest's
    /// `admin.actions`), so no grant is checked against this: an action is gated by its
    /// target's scope (`hypha_apps::Target::scope`), for the grant and the caller. This
    /// names what the caller needs besides.
    const SCOPE: Scope = Scope::AppsRead;
    const READ_ONLY: bool = false;
    type Request = AdminActionRequest;
    type Response = AdminActionResponse;

    /// `message` is 1 to 500 characters of plain text; `navigate` is a path within the
    /// app's page (one `/` first), as the bridge's `navigate` takes.
    fn check(_: &AdminActionRequest, response: &AdminActionResponse) -> Result<(), String> {
        if let Some(message) = &response.message {
            let len = message.chars().count();
            if message.trim().is_empty() || len > MAX_ACTION_MESSAGE {
                return Err(format!(
                    "message is 1 to {MAX_ACTION_MESSAGE} characters, not {len}"
                ));
            }
        }
        if let Some(path) = &response.navigate
            && (!path.starts_with('/') || path.starts_with("//") || path.len() > 2048)
        {
            return Err(format!(
                "navigate {path:?} isn't a path within the app's page"
            ));
        }
        Ok(())
    }
}

/// The objects the action was pressed on, as the admin passed them (checked against
/// the action's target).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = AdminActionContext))]
pub struct ActionContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub order_id: Option<OrderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub payment_id: Option<PaymentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub fulfillment_id: Option<FulfillmentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub product_id: Option<ProductId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub customer_id: Option<CustomerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub cart_id: Option<CartId>,
}

/// What Hypha sends: `POST <the action's url>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdminActionRequest {
    /// The action's `id` in the manifest.
    pub action: String,
    /// Where it was pressed (`order.payment`, `payment`, …).
    pub target: String,
    pub context: ActionContext,
    /// The form's values when the action has an `input_schema` (an object), as the
    /// admin sent them: check them against the schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false, value_type = Option<Object>))]
    pub input: Option<serde_json::Value>,
    /// The staff member who pressed it (`user_…`); absent for an API key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub staff: Option<UserId>,
    /// The app's scopes as far as the caller holds them, space-separated: what the
    /// caller may have the app do for them.
    pub scope: String,
}

/// What the app answers: what the admin shows next. Every field is optional; `{}` is
/// a plain success.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdminActionResponse {
    /// Plain text to show (a toast), 1 to 500 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub message: Option<String>,
    /// Whether the admin should reload what the action was pressed on.
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub refresh: bool,
    /// A path within the app's page to go to (`/orders/ord_…`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub navigate: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> AdminActionRequest {
        AdminActionRequest {
            action: "capture".to_string(),
            target: "payment".to_string(),
            context: ActionContext::default(),
            input: None,
            staff: None,
            scope: String::new(),
        }
    }

    fn answer(message: Option<&str>, navigate: Option<&str>) -> AdminActionResponse {
        AdminActionResponse {
            message: message.map(str::to_string),
            refresh: false,
            navigate: navigate.map(str::to_string),
        }
    }

    #[test]
    fn an_empty_answer_is_a_success() {
        let parsed: AdminActionResponse = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, AdminActionResponse::default());
        assert_eq!(RunAdminAction::check(&request(), &parsed), Ok(()));
    }

    #[test]
    fn a_message_is_plain_and_short() {
        let ok = answer(Some("Captured"), None);
        assert_eq!(RunAdminAction::check(&request(), &ok), Ok(()));
        let full = "x".repeat(MAX_ACTION_MESSAGE);
        assert_eq!(
            RunAdminAction::check(&request(), &answer(Some(&full), None)),
            Ok(())
        );
        let long = "x".repeat(MAX_ACTION_MESSAGE + 1);
        assert!(RunAdminAction::check(&request(), &answer(Some(&long), None)).is_err());
        assert!(RunAdminAction::check(&request(), &answer(Some(" "), None)).is_err());
    }

    #[test]
    fn navigate_stays_within_the_apps_page() {
        assert_eq!(
            RunAdminAction::check(&request(), &answer(None, Some("/orders/1"))),
            Ok(())
        );
        let path = |len: usize| format!("/{}", "x".repeat(len - 1));
        assert_eq!(
            RunAdminAction::check(&request(), &answer(None, Some(&path(2048)))),
            Ok(())
        );
        assert!(RunAdminAction::check(&request(), &answer(None, Some(&path(2049)))).is_err());
        for bad in ["https://evil.example", "//evil.example", "orders"] {
            assert!(
                RunAdminAction::check(&request(), &answer(None, Some(bad))).is_err(),
                "{bad}"
            );
        }
    }
}
