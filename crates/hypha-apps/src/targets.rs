//! Where an app's UI can appear (design note 0029, §5): a closed catalog of targets,
//! stable slot names any admin or storefront may implement. A frontend shows the
//! targets it supports and ignores the rest; manifests may name targets a later
//! version adds (they're listed nowhere until this catalog knows them).

use core::fmt;

use hypha_core::{CartId, CustomerId, FulfillmentId, OrderId, PaymentId, ProductId, Scope};

/// Which frontend shows a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Admin,
    Storefront,
}

/// A slot app UI fills. Names are part of the contract: never renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// Admin: a full page under the app (an embedded page).
    AppsPage,
    /// Admin: the app's settings (an embedded page, or the manifest's settings form).
    SettingsApp,
    /// Admin: blocks and actions on an order.
    OrderDetails,
    /// Admin: on an order's payment.
    OrderPayment,
    /// Admin: on an order's fulfilment.
    OrderFulfillment,
    /// Admin: on a product.
    ProductDetails,
    /// Admin: on a customer.
    CustomerDetails,
    /// Admin: buttons on a payment.
    Payment,
    /// Storefront: the payment step.
    CheckoutPayment,
    /// Storefront: on a product page.
    ProductBlock,
    /// Storefront: on the cart.
    CartBlock,
    /// Storefront: on an order's confirmation.
    OrderConfirmation,
}

/// What a target's context may carry: the IDs of the objects it's about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextKey {
    Order,
    Payment,
    Fulfillment,
    Product,
    Customer,
    Cart,
}

impl ContextKey {
    /// As the API and apps spell it (`order_id`).
    pub fn as_str(self) -> &'static str {
        match self {
            ContextKey::Order => "order_id",
            ContextKey::Payment => "payment_id",
            ContextKey::Fulfillment => "fulfillment_id",
            ContextKey::Product => "product_id",
            ContextKey::Customer => "customer_id",
            ContextKey::Cart => "cart_id",
        }
    }
}

impl Target {
    pub const ALL: [Target; 12] = [
        Target::AppsPage,
        Target::SettingsApp,
        Target::OrderDetails,
        Target::OrderPayment,
        Target::OrderFulfillment,
        Target::ProductDetails,
        Target::CustomerDetails,
        Target::Payment,
        Target::CheckoutPayment,
        Target::ProductBlock,
        Target::CartBlock,
        Target::OrderConfirmation,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Target::AppsPage => "apps.page",
            Target::SettingsApp => "settings.app",
            Target::OrderDetails => "order.details",
            Target::OrderPayment => "order.payment",
            Target::OrderFulfillment => "order.fulfillment",
            Target::ProductDetails => "product.details",
            Target::CustomerDetails => "customer.details",
            Target::Payment => "payment",
            Target::CheckoutPayment => "checkout.payment",
            Target::ProductBlock => "product.block",
            Target::CartBlock => "cart.block",
            Target::OrderConfirmation => "order.confirmation",
        }
    }

    /// The target `name` spells; `None` for one this catalog doesn't know.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == name)
    }

    pub fn surface(self) -> Surface {
        match self {
            Target::CheckoutPayment
            | Target::ProductBlock
            | Target::CartBlock
            | Target::OrderConfirmation => Surface::Storefront,
            _ => Surface::Admin,
        }
    }

    /// Whether admin pages (embedded) fill it.
    pub fn takes_pages(self) -> bool {
        matches!(self, Target::AppsPage | Target::SettingsApp)
    }

    /// Whether admin blocks (declarative UI trees) fill it.
    pub fn takes_blocks(self) -> bool {
        matches!(
            self,
            Target::OrderDetails
                | Target::OrderPayment
                | Target::OrderFulfillment
                | Target::ProductDetails
                | Target::CustomerDetails
        )
    }

    /// Whether admin actions (buttons) fill it.
    pub fn takes_actions(self) -> bool {
        self.takes_blocks() || self == Target::Payment
    }

    /// Whether storefront elements (web components) fill it.
    pub fn takes_elements(self) -> bool {
        self.surface() == Surface::Storefront
    }

    /// The context keys it's given: the first is required, the others optional.
    pub fn context(self) -> &'static [ContextKey] {
        use ContextKey as K;
        match self {
            Target::AppsPage | Target::SettingsApp => &[],
            Target::OrderDetails => &[K::Order],
            Target::OrderPayment => &[K::Order, K::Payment],
            Target::OrderFulfillment => &[K::Order, K::Fulfillment],
            Target::ProductDetails => &[K::Product],
            Target::CustomerDetails => &[K::Customer],
            Target::Payment => &[K::Payment, K::Order],
            Target::CheckoutPayment => &[K::Cart, K::Order],
            Target::ProductBlock => &[K::Product],
            Target::CartBlock => &[K::Cart],
            Target::OrderConfirmation => &[K::Order],
        }
    }

    /// The read scope the target's object needs: an app's grant must have it for its
    /// contributions there to be listed or called (design note 0029, §4.1 step 5), and
    /// a staff member must have it to use them (they're about that object).
    pub fn scope(self) -> Option<Scope> {
        match self {
            Target::AppsPage | Target::SettingsApp | Target::CheckoutPayment => None,
            Target::OrderDetails
            | Target::OrderPayment
            | Target::OrderFulfillment
            | Target::Payment
            | Target::OrderConfirmation => Some(Scope::OrdersRead),
            Target::ProductDetails | Target::ProductBlock => Some(Scope::ProductsRead),
            Target::CustomerDetails => Some(Scope::CustomersRead),
            Target::CartBlock => Some(Scope::CartsRead),
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The objects an extension is used on: the IDs a frontend passes (`?order_id=…`),
/// checked against the target's context.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtensionContext {
    pub order: Option<OrderId>,
    pub payment: Option<PaymentId>,
    pub fulfillment: Option<FulfillmentId>,
    pub product: Option<ProductId>,
    pub customer: Option<CustomerId>,
    pub cart: Option<CartId>,
}

impl ExtensionContext {
    /// The keys set, with their prefixed IDs, in `ContextKey` order.
    pub fn pairs(&self) -> Vec<(ContextKey, String)> {
        let mut pairs = Vec::new();
        let mut push = |key, value: Option<String>| {
            if let Some(value) = value {
                pairs.push((key, value));
            }
        };
        push(ContextKey::Order, self.order.map(|i| i.to_string()));
        push(ContextKey::Payment, self.payment.map(|i| i.to_string()));
        push(
            ContextKey::Fulfillment,
            self.fulfillment.map(|i| i.to_string()),
        );
        push(ContextKey::Product, self.product.map(|i| i.to_string()));
        push(ContextKey::Customer, self.customer.map(|i| i.to_string()));
        push(ContextKey::Cart, self.cart.map(|i| i.to_string()));
        pairs
    }

    /// The first ID set, for the call log's correlation.
    pub fn correlation(&self) -> Option<String> {
        self.pairs().into_iter().next().map(|(_, id)| id)
    }

    /// Whether this fits `target`: every key given is one it takes, and its first key
    /// (when it has any) is given. The broken rule otherwise, in words.
    pub fn check(&self, target: Target) -> Result<(), String> {
        let allowed = target.context();
        let given = self.pairs();
        if let Some((key, _)) = given.iter().find(|(k, _)| !allowed.contains(k)) {
            return Err(format!("{target} takes no {}", key.as_str()));
        }
        if let Some(first) = allowed.first()
            && !given.iter().any(|(k, _)| k == first)
        {
            return Err(format!("{target} needs {}", first.as_str()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn every_target_parses_from_its_name() {
        for target in Target::ALL {
            assert_eq!(Target::parse(target.as_str()), Some(target));
        }
        assert_eq!(Target::parse("order.paymnet"), None);
    }

    #[test]
    fn targets_take_their_kind_of_contribution() {
        assert!(Target::AppsPage.takes_pages() && !Target::AppsPage.takes_blocks());
        assert!(Target::OrderPayment.takes_blocks() && Target::OrderPayment.takes_actions());
        assert!(Target::Payment.takes_actions() && !Target::Payment.takes_blocks());
        assert!(Target::CheckoutPayment.takes_elements());
        assert!(!Target::OrderDetails.takes_elements());
    }

    #[test]
    fn a_context_has_the_targets_keys_only_and_its_first() {
        let order = OrderId::from_uuid(Uuid::from_u128(1));
        let context = ExtensionContext {
            order: Some(order),
            ..Default::default()
        };
        assert_eq!(context.check(Target::OrderPayment), Ok(()));
        assert_eq!(
            context.check(Target::ProductDetails),
            Err("product.details takes no order_id".to_string())
        );
        assert_eq!(
            ExtensionContext::default().check(Target::OrderDetails),
            Err("order.details needs order_id".to_string())
        );
        assert_eq!(ExtensionContext::default().check(Target::AppsPage), Ok(()));
        assert_eq!(context.correlation(), Some(order.to_string()));
    }
}
