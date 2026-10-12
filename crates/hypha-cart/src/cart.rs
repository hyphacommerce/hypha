use hypha_core::{CartId, MarketId, ProductId, VariantId};

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

/// A cart belongs to one market, which fixes its currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cart {
    id: CartId,
    market: MarketId,
    lines: Vec<CartLine>,
}

impl Cart {
    pub fn new(id: CartId, market: MarketId) -> Self {
        Self {
            id,
            market,
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

    pub fn lines(&self) -> &[CartLine] {
        &self.lines
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
}
