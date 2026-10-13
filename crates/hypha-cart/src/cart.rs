use hypha_core::{CartId, CountryCode, MarketId, ProductId, VariantId, Version, Versioned};

use crate::{error::CartError, quantity::Quantity};

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

/// A cart ships to one country, which picked its market (and so its currency) when the
/// cart was created, and picks its tax rates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cart {
    id: CartId,
    version: Version,
    market: MarketId,
    country: CountryCode,
    lines: Vec<CartLine>,
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
        }
    }

    /// Adds a line, or increases the quantity if the variant is already in the cart.
    pub fn add_line(
        &mut self,
        product: ProductId,
        variant: VariantId,
        quantity: Quantity,
    ) -> Result<(), CartError> {
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
        let line = self
            .lines
            .iter_mut()
            .find(|l| l.variant == variant)
            .ok_or(CartError::LineNotFound(variant))?;
        line.quantity = quantity;
        Ok(())
    }

    /// Returns `false` if the variant wasn't in the cart.
    pub fn remove_line(&mut self, variant: VariantId) -> bool {
        let before = self.lines.len();
        self.lines.retain(|l| l.variant != variant);
        self.lines.len() != before
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
        }
    }

    /// Rebuilds a cart from storage: quantities must be at least 1 and each variant may
    /// appear on one line only (`add_line` merges, so a second line means bad data).
    pub fn restore(s: CartSnapshot) -> Result<Cart, CartError> {
        let mut lines: Vec<CartLine> = Vec::with_capacity(s.lines.len());
        for line in s.lines {
            if lines.iter().any(|l| l.variant == line.variant) {
                return Err(CartError::DuplicateLine(line.variant));
            }
            lines.push(CartLine {
                product: line.product,
                variant: line.variant,
                quantity: Quantity::new(line.quantity)?,
            });
        }
        Ok(Cart {
            id: s.id,
            version: s.version,
            market: s.market,
            country: s.country,
            lines,
        })
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

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
            Err(CartError::QuantityOverflow)
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
    fn remove_line_reports_whether_it_was_there() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(1)).unwrap();
        assert!(c.remove_line(vid(1)));
        assert!(!c.remove_line(vid(1)));
        assert!(c.lines().is_empty());
    }

    #[test]
    fn restore_round_trips() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(2)).unwrap();
        c.add_line(pid(), vid(2), qty(1)).unwrap();
        c.set_version(Version::new(5));
        assert_eq!(Cart::restore(c.snapshot()), Ok(c));
    }

    #[test]
    fn restore_rejects_zero_quantity() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(2)).unwrap();
        let mut s = c.snapshot();
        s.lines[0].quantity = 0;
        assert_eq!(Cart::restore(s), Err(CartError::ZeroQuantity));
    }

    #[test]
    fn restore_rejects_duplicate_variant_line() {
        let mut c = cart();
        c.add_line(pid(), vid(1), qty(2)).unwrap();
        let mut s = c.snapshot();
        s.lines.push(s.lines[0].clone());
        assert_eq!(Cart::restore(s), Err(CartError::DuplicateLine(vid(1))));
    }
}
