use hypha_core::{CartId, OrderId, PaymentId, PaymentMethodId, RepoError};

use crate::{Payment, PaymentMethod};

/// Uniqueness rules the payment repos enforce, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const PAYMENT_METHOD_ID: &str = "payment_method_id";
    pub const PAYMENT_ID: &str = "payment_id";
    /// An open (`pending` or `authorized`) payment uses the method: deleting it is
    /// refused (`RepoError::Reference`), checked in the same statement as the delete.
    pub const OPEN_PAYMENT_METHOD: &str = "open_payment_method";
}

#[allow(async_fn_in_trait)]
pub trait PaymentMethodRepo {
    async fn get(&self, id: PaymentMethodId) -> Result<Option<PaymentMethod>, RepoError>;
    /// Every method, by position and then ID: the order shoppers see them in. A store
    /// has a handful.
    async fn list(&self) -> Result<Vec<PaymentMethod>, RepoError>;
    /// Stores a new method (version 1). Enforces `PAYMENT_METHOD_ID`.
    async fn insert(&self, method: &mut PaymentMethod) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, method: &mut PaymentMethod) -> Result<(), RepoError>;
    /// Removes `method` at the version it was loaded at, or fails with `StaleVersion`
    /// (changed or gone since). Refused with `Reference(OPEN_PAYMENT_METHOD)` while an
    /// open (`pending` or `authorized`) payment uses it, in the same step as the delete,
    /// so a checkout racing it can't strand a payment (design note 0027, §7). Its other
    /// payments keep its ID, name and provider.
    async fn delete(&self, method: &PaymentMethod) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait PaymentRepo {
    async fn get(&self, id: PaymentId) -> Result<Option<Payment>, RepoError>;
    /// Every payment of `order`, newest first (by ID). An order has a handful.
    async fn list_for_order(&self, order: OrderId) -> Result<Vec<Payment>, RepoError>;
    /// Every payment started for `cart`, newest first (by ID): the ones checkout started
    /// before the order (design note 0031), and the order's own once placed. A cart has
    /// a handful.
    async fn list_for_cart(&self, cart: CartId) -> Result<Vec<Payment>, RepoError>;
    /// Stores a new payment (version 1). Enforces `PAYMENT_ID`. Workflows insert
    /// through `StagePayments` instead, with the order.
    async fn insert(&self, payment: &mut Payment) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, payment: &mut Payment) -> Result<(), RepoError>;
}

/// The newest payment of `order` (its payments sort newest first): the one checkout
/// started, what a storefront shows.
pub async fn newest_payment<S: HasPayments>(
    ports: &S,
    order: OrderId,
) -> Result<Option<Payment>, RepoError> {
    Ok(ports
        .payments()
        .list_for_order(order)
        .await?
        .into_iter()
        .next())
}

/// The newest payment started for `cart`: the one checkout completes with.
pub async fn newest_cart_payment<S: HasPayments>(
    ports: &S,
    cart: CartId,
) -> Result<Option<Payment>, RepoError> {
    Ok(ports
        .payments()
        .list_for_cart(cart)
        .await?
        .into_iter()
        .next())
}

/// Stages payment writes in a `hypha_core::HasBatch` batch; the adapter's batch type
/// implements it.
pub trait StagePayments {
    /// Stages what `PaymentRepo::insert` would write; `payment` is at version 1 once the
    /// batch commits.
    fn insert_payment(&mut self, payment: &mut Payment);
    /// Stages what `PaymentRepo::update` would write, and moves `payment` to the version
    /// it has once the batch commits (stale then: `StaleVersion`, nothing written).
    fn update_payment(&mut self, payment: &mut Payment);
}

/// Whatever holds the `PaymentMethodRepo`: a host's per-request ports, a test world.
pub trait HasPaymentMethods {
    type PaymentMethods: PaymentMethodRepo;
    fn payment_methods(&self) -> &Self::PaymentMethods;
}

/// Whatever holds the `PaymentRepo`.
pub trait HasPayments {
    type Payments: PaymentRepo;
    fn payments(&self) -> &Self::Payments;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasPaymentMethods + ?Sized> HasPaymentMethods for &T {
    type PaymentMethods = T::PaymentMethods;
    fn payment_methods(&self) -> &Self::PaymentMethods {
        (**self).payment_methods()
    }
}

impl<T: HasPayments + ?Sized> HasPayments for &T {
    type Payments = T::Payments;
    fn payments(&self) -> &Self::Payments {
        (**self).payments()
    }
}
