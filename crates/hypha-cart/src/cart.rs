use hypha_core::{
    Address, AddressParts, CartId, ChannelId, CountryCode, CustomerId, Email, MarketId, OrderId,
    PaymentMethodId, Price, ProductId, Quantity, ShippingGroupId, ShippingOptionId, Stamped,
    Stamps, Timestamp, VariantId, Version, Versioned,
};
use hypha_discounts::MAX_CODES_PER_CART;
use hypha_shipping::ProviderKey;

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

/// The buyer's shipping choice for the cart's group: an option, and for one priced by a
/// provider, the quote they were shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedShipping {
    pub option: ShippingOptionId,
    pub quote: Option<StoredQuote>,
}

/// A provider's price for the cart as it was when quoted. Built-in prices aren't stored:
/// they follow the cart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredQuote {
    pub amount: Price,
    pub provider: ProviderKey,
    pub service: String,
    pub reference: Option<String>,
    pub quoted_at: Timestamp,
}

impl StoredQuote {
    /// How long a quote holds before checkout asks the provider again.
    pub const VALID_FOR_SECS: i64 = 15 * 60;

    pub fn expires_at(&self) -> Timestamp {
        self.quoted_at.plus_secs(Self::VALID_FOR_SECS)
    }
}

/// Whether a cart can still change.
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
/// Its buyer is an optional `email` and `customer`, like Shopify's `buyerIdentity`;
/// checkout needs at least the email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cart {
    id: CartId,
    version: Version,
    /// When it was stored and last changed, set by the adapter.
    stamps: Option<Stamps>,
    market: MarketId,
    country: CountryCode,
    /// The sales channel it was made in: its storefront's. It sells only what that channel
    /// publishes, and its order records it.
    channel: ChannelId,
    lines: Vec<CartLine>,
    email: Option<Email>,
    customer: Option<CustomerId>,
    status: CartStatus,
    /// In `country`.
    shipping_address: Option<Address>,
    /// The choice for the cart's one group of shippable lines.
    shipping: Option<SelectedShipping>,
    /// How the buyer will pay: checked when chosen and again at checkout.
    payment_method: Option<PaymentMethodId>,
    /// "Email me with news and offers": checkout records the consent when it's ticked;
    /// unticked never unsubscribes anyone.
    marketing_opt_in: bool,
    /// Discount codes entered, normalised, in the order entered: at most
    /// `MAX_CODES_PER_CART`. A code stays while it doesn't apply, so it applies once the
    /// cart qualifies; pricing says where each stands.
    discount_codes: Vec<String>,
}

impl Versioned for Cart {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Cart {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Cart {
    pub fn new(id: CartId, market: MarketId, country: CountryCode, channel: ChannelId) -> Self {
        Self {
            id,
            version: Version::NEW,
            stamps: None,
            market,
            country,
            channel,
            lines: Vec::new(),
            email: None,
            customer: None,
            status: CartStatus::Open,
            shipping_address: None,
            shipping: None,
            payment_method: None,
            marketing_opt_in: false,
            discount_codes: Vec::new(),
        }
    }

    /// Erasure of a cart that was checked out: the buyer's email becomes `placeholder` and
    /// the address goes; the lines, totals and link to the customer stay. Open carts are
    /// deleted instead. Not bound to the cart being open.
    pub fn redact(&mut self, placeholder: Email) -> bool {
        let mut changed = false;
        if self.email.as_ref().is_some_and(|e| *e != placeholder) {
            self.email = Some(placeholder);
            changed = true;
        }
        if self.shipping_address.is_some() {
            self.shipping_address = None;
            self.shipping = None;
            changed = true;
        }
        changed
    }

    /// Holds `code` (normalised; commands check it's a live code first). Returns whether
    /// it's new: a code held already stays where it is.
    pub fn add_discount_code(&mut self, code: String) -> Result<bool, CartError> {
        self.ensure_open()?;
        if self.discount_codes.contains(&code) {
            return Ok(false);
        }
        if self.discount_codes.len() >= MAX_CODES_PER_CART {
            return Err(CartError::TooManyDiscountCodes {
                max: MAX_CODES_PER_CART,
            });
        }
        self.discount_codes.push(code);
        Ok(true)
    }

    /// Lets go of `code` (normalised).
    pub fn remove_discount_code(&mut self, code: &str) -> Result<(), CartError> {
        self.ensure_open()?;
        let before = self.discount_codes.len();
        self.discount_codes.retain(|c| c != code);
        if self.discount_codes.len() == before {
            return Err(CartError::DiscountCodeNotOnCart(code.to_string()));
        }
        Ok(())
    }

    /// Normalised, in the order entered.
    pub fn discount_codes(&self) -> &[String] {
        &self.discount_codes
    }

    /// Ticks or unticks the marketing box.
    pub fn set_marketing_opt_in(&mut self, opt_in: bool) -> Result<(), CartError> {
        self.ensure_open()?;
        self.marketing_opt_in = opt_in;
        Ok(())
    }

    pub fn marketing_opt_in(&self) -> bool {
        self.marketing_opt_in
    }

    /// Every change goes through this first: a checked-out cart is the order's history.
    /// Commands also call it right after loading, so a completed cart is reported as
    /// such before any other check (an unknown line or variant).
    pub(crate) fn ensure_open(&self) -> Result<(), CartError> {
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

    /// A linked customer is who the order is for, so their cart keeps their email: clearing
    /// it fails until the customer is unlinked, and changing it is refused (setting the
    /// same address again is fine).
    pub fn set_email(&mut self, email: Option<Email>) -> Result<(), CartError> {
        self.ensure_open()?;
        if self.customer.is_some() {
            match &email {
                None => return Err(CartError::EmailRequired),
                Some(new) if Some(new) != self.email.as_ref() => {
                    return Err(CartError::CustomerEmailFixed);
                }
                Some(_) => {}
            }
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

    /// Where the cart ships; it must be in the cart's country. A new address drops the
    /// shipping choice: what's offered depends on the destination.
    pub fn set_shipping_address(&mut self, address: Option<Address>) -> Result<(), CartError> {
        self.ensure_open()?;
        if let Some(a) = &address
            && a.country() != self.country
        {
            return Err(CartError::AddressCountryMismatch {
                address: a.country(),
                cart: self.country,
            });
        }
        if address != self.shipping_address {
            self.shipping = None;
        }
        self.shipping_address = address;
        Ok(())
    }

    /// The buyer's choice; picking one needs an address. Commands check the option is
    /// offered (`select_shipping`).
    pub fn select_shipping(&mut self, selected: Option<SelectedShipping>) -> Result<(), CartError> {
        self.ensure_open()?;
        if selected.is_some() && self.shipping_address.is_none() {
            return Err(CartError::ShippingAddressRequired);
        }
        self.shipping = selected;
        Ok(())
    }

    /// The buyer's payment method; `None` clears it. Commands check it's offered
    /// (`hypha_checkout::set_payment_method`).
    pub fn set_payment_method(&mut self, method: Option<PaymentMethodId>) -> Result<(), CartError> {
        self.ensure_open()?;
        self.payment_method = method;
        Ok(())
    }

    /// The ID of the cart's group of shippable lines. One group for now, named after the
    /// cart, so it stays the same however its lines change.
    pub fn shipping_group(&self) -> ShippingGroupId {
        ShippingGroupId::from_uuid(self.id.as_uuid())
    }

    /// A provider's quote was for the lines as they were: changing them drops the choice.
    /// A built-in price simply follows the lines.
    fn lines_changed(&mut self) {
        if self.shipping.as_ref().is_some_and(|s| s.quote.is_some()) {
            self.shipping = None;
        }
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
        self.lines_changed();
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
        self.lines_changed();
        Ok(())
    }

    pub fn remove_line(&mut self, variant: VariantId) -> Result<(), CartError> {
        self.ensure_open()?;
        let before = self.lines.len();
        self.lines.retain(|l| l.variant != variant);
        if self.lines.len() == before {
            return Err(CartError::LineNotFound(variant));
        }
        self.lines_changed();
        Ok(())
    }

    pub fn id(&self) -> CartId {
        self.id
    }

    pub fn market(&self) -> MarketId {
        self.market
    }

    /// The sales channel it sells through.
    pub fn channel(&self) -> ChannelId {
        self.channel
    }

    /// Whether a caller in `channel` may use the cart: a storefront only its own channel's
    /// carts; staff and secret keys, in no channel, every cart.
    pub fn visible_in(&self, channel: Option<ChannelId>) -> bool {
        channel.is_none_or(|c| c == self.channel)
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

    pub fn shipping_address(&self) -> Option<&Address> {
        self.shipping_address.as_ref()
    }

    pub fn shipping(&self) -> Option<&SelectedShipping> {
        self.shipping.as_ref()
    }

    pub fn payment_method(&self) -> Option<PaymentMethodId> {
        self.payment_method
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

/// Plain-data form of a `Cart` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartSnapshot {
    pub id: CartId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub market: MarketId,
    pub country: CountryCode,
    pub channel: ChannelId,
    pub lines: Vec<CartLineSnapshot>,
    pub email: Option<Email>,
    pub customer: Option<CustomerId>,
    /// `Some` once checked out (`CartStatus::Completed`).
    pub order: Option<OrderId>,
    /// Checked by `Address::new` on restore.
    pub shipping_address: Option<AddressParts>,
    pub shipping: Option<SelectedShipping>,
    pub payment_method: Option<PaymentMethodId>,
    pub marketing_opt_in: bool,
    /// Normalised; at most `MAX_CODES_PER_CART`, each once.
    pub discount_codes: Vec<String>,
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
            stamps: self.stamps,
            market: self.market,
            country: self.country,
            channel: self.channel,
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
            shipping_address: self.shipping_address.as_ref().map(|a| a.parts().clone()),
            shipping: self.shipping.clone(),
            payment_method: self.payment_method,
            marketing_opt_in: self.marketing_opt_in,
            discount_codes: self.discount_codes.clone(),
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
        let shipping_address = s.shipping_address.map(Address::new).transpose()?;
        if let Some(a) = &shipping_address
            && a.country() != s.country
        {
            return Err(CartError::AddressCountryMismatch {
                address: a.country(),
                cart: s.country,
            }
            .into());
        }
        if s.shipping.is_some() && shipping_address.is_none() {
            return Err(CartError::ShippingAddressRequired.into());
        }
        if s.discount_codes.len() > MAX_CODES_PER_CART {
            return Err(CartError::TooManyDiscountCodes {
                max: MAX_CODES_PER_CART,
            }
            .into());
        }
        let mut seen = std::collections::HashSet::new();
        if let Some(twice) = s.discount_codes.iter().find(|c| !seen.insert(*c)) {
            return Err(RestoreError::DuplicateDiscountCode(twice.clone()));
        }
        Ok(Cart {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            market: s.market,
            country: s.country,
            channel: s.channel,
            lines,
            email: s.email,
            customer: s.customer,
            status: match s.order {
                None => CartStatus::Open,
                Some(order) => CartStatus::Completed { order },
            },
            shipping_address,
            shipping: s.shipping,
            payment_method: s.payment_method,
            marketing_opt_in: s.marketing_opt_in,
            discount_codes: s.discount_codes,
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
            ChannelId::from_uuid(Uuid::from_u128(500)),
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
        assert_eq!(
            c.set_email(Some(Email::new("other@example.com").unwrap())),
            Err(CartError::CustomerEmailFixed),
            "a customer's cart keeps their email"
        );
        assert_eq!(
            c.set_email(c.email().cloned()),
            Ok(()),
            "the same address is fine"
        );
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
