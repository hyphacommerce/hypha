use hypha_core::{
    CartId, CountryCode, CustomerId, Email, MarketId, OrderId, ProductId, Quantity, VariantId,
    Version, Versioned,
};

use crate::error::{CartError, RestoreError};

/// A variant and how many of it. Prices are not stored: they are resolved on read,
/// so a cart always reflects current prices. Snapshotting belongs to orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CartLine {
    product: ProductId,
    variant: VariantId,
    quantity: Quantity,
}

impl CartLine {
    pub fn product(&self) -> ProductId {
        self.product
    }

    pub fn variant(&self) -> VariantId {
        self.variant
    }

    pub fn quantity(&self) -> Quantity {
        self.quantity
    }
}

/// Whether a cart can still change (design note 0007, O6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CartStatus {
    Open,
    /// Checked out as `order`. Nothing about the cart changes after this.
    Completed {
        order: OrderId,
    },
}

/// A cart ships to one country, which picked its market (and so its currency) when the
/// cart was created, and picks its tax rates.
///
/// Its buyer is an optional `email` and `customer`, like Shopify's `buyerIdentity`
/// (design note 0005, C5); checkout needs at least the email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cart {
    id: CartId,
    version: Version,
    market: MarketId,
    country: CountryCode,
    lines: Vec<CartLine>,
    email: Option<Email>,
    customer: Option<CustomerId>,
    status: CartStatus,
}

impl Versioned for Cart {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Cart {
    pub fn new(id: CartId, market: MarketId, country: CountryCode) -> Self {
        Self {
            id,
            version: Version::NEW,
            market,
            country,
            lines: Vec::new(),
            email: None,
            customer: None,
            status: CartStatus::Open,
        }
    }

    /// Every change goes through this first: a checked-out cart is the order's history.
    fn ensure_open(&self) -> Result<(), CartError> {
        match self.status {
            CartStatus::Open => Ok(()),
            CartStatus::Completed { order } => Err(CartError::Completed { order }),
        }
    }

    /// Marks the cart checked out as `order`; only checkout calls this.
    pub fn complete(&mut self, order: OrderId) -> Result<(), CartError> {
        self.ensure_open()?;
        self.status = CartStatus::Completed { order };
        Ok(())
    }

    /// A linked customer is who the order is for, so their cart keeps an email: clearing
    /// it fails until the customer is unlinked.
    pub fn set_email(&mut self, email: Option<Email>) -> Result<(), CartError> {
        self.ensure_open()?;
        if email.is_none() && self.customer.is_some() {
            return Err(CartError::EmailRequired);
        }
        self.email = email;
        Ok(())
    }

    /// Links the cart to a customer and takes their email; `None` unlinks it and keeps
    /// the email.
    pub fn set_customer(&mut self, customer: Option<(CustomerId, Email)>) -> Result<(), CartError> {
        self.ensure_open()?;
        match customer {
            Some((id, email)) => {
                self.customer = Some(id);
                self.email = Some(email);
            }
            None => self.customer = None,
        }
        Ok(())
    }

    /// Adds a line, or increases the quantity if the variant is already in the cart.
    pub fn add_line(
        &mut self,
        product: ProductId,
        variant: VariantId,
        quantity: Quantity,
    ) -> Result<(), CartError> {
        self.ensure_open()?;
        if let Some(line) = self.lines.iter_mut().find(|l| l.variant == variant) {
            line.quantity = line.quantity.checked_add(quantity)?;
        } else {
            self.lines.push(CartLine {
                product,
                variant,
                quantity,
            });
        }
        Ok(())
    }

    pub fn set_quantity(
        &mut self,
        variant: VariantId,
        quantity: Quantity,
    ) -> Result<(), CartError> {
        self.ensure_open()?;
        let line = self
            .lines
            .iter_mut()
            .find(|l| l.variant == variant)
            .ok_or(CartError::LineNotFound(variant))?;
        line.quantity = quantity;
        Ok(())
    }

    pub fn remove_line(&mut self, variant: VariantId) -> Result<(), CartError> {
        self.ensure_open()?;
        let before = self.lines.len();
        self.lines.retain(|l| l.variant != variant);
        if self.lines.len() == before {
            return Err(CartError::LineNotFound(variant));
        }
        Ok(())
    }

    pub fn id(&self) -> CartId {
        self.id
    }

    pub fn market(&self) -> MarketId {
        self.market
    }

    pub fn country(&self) -> CountryCode {
        self.country
    }

    pub fn lines(&self) -> &[CartLine] {
        &self.lines
    }

    pub fn email(&self) -> Option<&Email> {
        self.email.as_ref()
    }

    pub fn customer(&self) -> Option<CustomerId> {
        self.customer
    }

    pub fn status(&self) -> CartStatus {
        self.status
    }

    /// The line holding `variant`.
    pub fn line(&self, variant: VariantId) -> Result<CartLine, CartError> {
        self.lines
            .iter()
            .find(|l| l.variant == variant)
            .copied()
            .ok_or(CartError::LineNotFound(variant))
    }
}

/// Plain-data form of a `Cart` for storage adapters (design note 0001, 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartSnapshot {
    pub id: CartId,
    pub version: Version,
    pub market: MarketId,
    pub country: CountryCode,
    pub lines: Vec<CartLineSnapshot>,
    pub email: Option<Email>,
    pub customer: Option<CustomerId>,
    /// `Some` once checked out (`CartStatus::Completed`).
    pub order: Option<OrderId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartLineSnapshot {
    pub product: ProductId,
    pub variant: VariantId,
    pub quantity: u32,
}

impl Cart {
    pub fn snapshot(&self) -> CartSnapshot {
        CartSnapshot {
            id: self.id,
            version: self.version,
            market: self.market,
            country: self.country,
            lines: self
                .lines
                .iter()
                .map(|l| CartLineSnapshot {
                    product: l.product,
                    variant: l.variant,
                    quantity: l.quantity.value(),
                })
                .collect(),
            email: self.email.clone(),
            customer: self.customer,
            order: match self.status {
                CartStatus::Open => None,
                CartStatus::Completed { order } => Some(order),
            },
        }
    }

    /// Rebuilds a cart from storage: quantities must be at least 1 and each variant may
    /// appear on one line only (`add_line` merges, so a second line means bad data).
    /// A cart with a customer has an email.
    pub fn restore(s: CartSnapshot) -> Result<Cart, RestoreError> {
        if s.customer.is_some() && s.email.is_none() {
            return Err(CartError::EmailRequired.into());
        }
        let mut lines: Vec<CartLine> = Vec::with_capacity(s.lines.len());
        for line in s.lines {
            if lines.iter().any(|l| l.variant == line.variant) {
                return Err(RestoreError::DuplicateLine(line.variant));
            }
            lines.push(CartLine {
                product: line.product,
                variant: line.variant,
                quantity: Quantity::new(line.quantity).map_err(CartError::from)?,
            });
        }
        Ok(Cart {
            id: s.id,
            version: s.version,
            market: s.market,
            country: s.country,
            lines,
            email: s.email,
            customer: s.customer,
            status: match s.order {
                None => CartStatus::Open,
                Some(order) => CartStatus::Completed { order },
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use hypha_core::QuantityError;

    use super::*;

    fn cart() -> Cart {
        Cart::new(
            CartId::from_uuid(Uuid::from_u128(1)),
            MarketId::from_uuid(Uuid::from_u128(2)),
            CountryCode::new("HR").unwrap(),
        )
    }

    fn pid() -> ProductId {
        ProductId::from_uuid(Uuid::from_u128(3))
    }

    fn vid(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn qty(n: u32) -> Quantity {
        Quantity::new(n).unwrap()
    }

    #[test]
    fn new_cart_is_empty() {
        assert!(cart().lines().is_empty());
    }

    #[test]
    fn add_line_appends_new_variant() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(2)).unwrap();
        c.add_line(pid(), vid(2), qty(1)).unwrap();
        let variants: Vec<_> = c.lines().iter().map(|l| l.variant()).collect();
        assert_eq!(variants, vec![vid(1), vid(2)]);
    }

    #[test]
    fn add_line_merges_same_variant() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(2)).unwrap();
        c.add_line(pid(), vid(1), qty(3)).unwrap();
        assert_eq!(c.lines().len(), 1);
        assert_eq!(c.lines()[0].quantity(), qty(5));
    }

    #[test]
    fn setting_a_customer_takes_their_email_and_unsetting_keeps_it() {
        let mut c = cart();
        let ana = Email::new("ana@example.hr").unwrap();
        let customer = CustomerId::from_uuid(Uuid::from_u128(5));
        c.set_email(Some(Email::new("old@example.hr").unwrap()))
            .unwrap();
        c.set_customer(Some((customer, ana.clone()))).unwrap();
        assert_eq!((c.customer(), c.email()), (Some(customer), Some(&ana)));
        c.set_customer(None).unwrap();
        assert_eq!((c.customer(), c.email()), (None, Some(&ana)));
    }

    #[test]
    fn a_linked_customer_keeps_the_email() {
        let mut c = cart();
        let ana = Email::new("ana@example.hr").unwrap();
        c.set_customer(Some((
            CustomerId::from_uuid(Uuid::from_u128(5)),
            ana.clone(),
        )))
        .unwrap();
        assert_eq!(c.set_email(None), Err(CartError::EmailRequired));
        assert_eq!(c.email(), Some(&ana));

        let mut s = c.snapshot();
        s.email = None;
        assert_eq!(Cart::restore(s), Err(CartError::EmailRequired.into()));
    }

    #[test]
    fn line_finds_the_variant() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(2)).unwrap();
        assert_eq!(c.line(vid(1)).map(|l| l.quantity()), Ok(qty(2)));
        assert_eq!(c.line(vid(2)), Err(CartError::LineNotFound(vid(2))));
    }

    #[test]
    fn add_line_rejects_overflow_and_keeps_quantity() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(u32::MAX)).unwrap();
        assert_eq!(
            c.add_line(pid(), vid(1), qty(1)),
            Err(CartError::Quantity(QuantityError::Overflow))
        );
        assert_eq!(c.lines()[0].quantity(), qty(u32::MAX));
    }

    #[test]
    fn set_quantity_replaces_it() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(2)).unwrap();
        assert_eq!(c.set_quantity(vid(1), qty(7)), Ok(()));
        assert_eq!(c.lines()[0].quantity(), qty(7));
    }

    #[test]
    fn set_quantity_rejects_missing_line() {
        assert_eq!(
            cart().set_quantity(vid(1), qty(1)),
            Err(CartError::LineNotFound(vid(1)))
        );
    }

    #[test]
    fn remove_line_reports_a_missing_line() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(1)).unwrap();
        assert_eq!(c.remove_line(vid(1)), Ok(()));
        assert_eq!(c.remove_line(vid(1)), Err(CartError::LineNotFound(vid(1))));
        assert!(c.lines().is_empty());
    }

    fn order() -> OrderId {
        OrderId::from_uuid(Uuid::from_u128(9))
    }

    #[test]
    fn a_completed_cart_refuses_every_change() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(1)).unwrap();
        c.complete(order()).unwrap();
        assert_eq!(c.status(), CartStatus::Completed { order: order() });

        let completed = Err(CartError::Completed { order: order() });
        assert_eq!(c.add_line(pid(), vid(2), qty(1)), completed);
        assert_eq!(c.set_quantity(vid(1), qty(2)), completed);
        assert_eq!(c.remove_line(vid(1)), completed);
        assert_eq!(c.set_email(None), completed);
        assert_eq!(c.set_customer(None), completed);
        assert_eq!(c.complete(order()), completed);
        assert_eq!(c.lines().len(), 1);
    }

    #[test]
    fn restore_round_trips() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(2)).unwrap();
        c.add_line(pid(), vid(2), qty(1)).unwrap();
        c.set_version(Version::new(5));
        assert_eq!(Cart::restore(c.snapshot()), Ok(c.clone()));

        c.complete(order()).unwrap();
        assert_eq!(Cart::restore(c.snapshot()), Ok(c));
    }

    #[test]
    fn restore_rejects_zero_quantity() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(2)).unwrap();
        let mut s = c.snapshot();
        s.lines[0].quantity = 0;
        assert_eq!(
            Cart::restore(s),
            Err(CartError::Quantity(QuantityError::Zero).into())
        );
    }

    #[test]
    fn restore_rejects_duplicate_variant_line() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(2)).unwrap();
        let mut s = c.snapshot();
        s.lines.push(s.lines[0].clone());
        assert_eq!(Cart::restore(s), Err(RestoreError::DuplicateLine(vid(1))));
    }
}
