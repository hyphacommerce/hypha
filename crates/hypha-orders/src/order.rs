use std::{collections::HashSet, fmt, str::FromStr};

use hypha_core::{
    CartId, CountryCode, Currency, CustomerId, Email, MarketId, Money, OrderId, OrderLineId, Price,
    ProductId, Quantity, TaxClassId, TaxRate, TaxRegionId, Timestamp, VariantId, Version,
    Versioned, required_text,
};

use crate::OrderError;

/// An order's human number (`#1001`) from the `OrderNumbers` port: unique and
/// increasing, possibly with gaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrderNumber(i64);

impl OrderNumber {
    pub fn new(n: i64) -> Result<Self, OrderError> {
        if n < 1 {
            return Err(OrderError::InvalidNumber(n));
        }
        Ok(Self(n))
    }

    pub fn value(self) -> i64 {
        self.0
    }
}

impl fmt::Display for OrderNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Where the money is (design note 0007, O3). Payment providers move it later
/// (`Authorized`, `Refunded`, …); for now an order is placed `Pending` and can be marked
/// `Paid` (e.g. a bank transfer arrived).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinancialStatus {
    Pending,
    Paid,
}

impl FinancialStatus {
    /// The same string used in JSON and storage, e.g. "paid".
    pub fn as_str(&self) -> &'static str {
        match self {
            FinancialStatus::Pending => "pending",
            FinancialStatus::Paid => "paid",
        }
    }
}

impl fmt::Display for FinancialStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FinancialStatus {
    /// The text that didn't match a status.
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "pending" => Ok(FinancialStatus::Pending),
            "paid" => Ok(FinancialStatus::Paid),
            other => Err(other.to_string()),
        }
    }
}

/// Where the goods are (design note 0007, O3). Fulfillment comes later; until then
/// every order is `Unfulfilled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FulfillmentStatus {
    Unfulfilled,
}

impl FulfillmentStatus {
    /// The same string used in JSON and storage, e.g. "unfulfilled".
    pub fn as_str(&self) -> &'static str {
        match self {
            FulfillmentStatus::Unfulfilled => "unfulfilled",
        }
    }
}

impl fmt::Display for FulfillmentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FulfillmentStatus {
    /// The text that didn't match a status.
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "unfulfilled" => Ok(FulfillmentStatus::Unfulfilled),
            other => Err(other.to_string()),
        }
    }
}

/// Which rule produced a line's tax, copied from the cart's tax line so orders don't
/// depend on the tax crate (design note 0007, O4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderTaxSource {
    RateTable { region: TaxRegionId },
}

/// A line's tax as it was charged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderTax {
    pub rate: TaxRate,
    /// The class whose rate applied; `None` for the region's default rate.
    pub class: Option<TaxClassId>,
    pub amount: Money,
    pub source: OrderTaxSource,
}

/// One line as it was bought: names, prices and tax are copies, so the order reads the
/// same after the product, its prices or the tax rates change. A plain value; `Order`
/// checks its lines when it is placed or restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderLine {
    /// Also the reference of the stock reserved for this line (design note 0004, I8).
    pub id: OrderLineId,
    pub product: ProductId,
    pub variant: VariantId,
    pub quantity: Quantity,
    pub product_title: String,
    /// The variant's option values ("M / Blue"); `None` for a simple product.
    pub variant_title: Option<String>,
    pub sku: Option<String>,
    pub unit_price: Price,
    /// `unit_price` × `quantity`.
    pub total: Money,
    /// `None` when the cart's country had no tax rates.
    pub tax: Option<OrderTax>,
}

/// What checkout knows when it places an order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOrder {
    pub id: OrderId,
    pub number: OrderNumber,
    pub placed_at: Timestamp,
    pub cart: CartId,
    pub market: MarketId,
    pub currency: Currency,
    pub country: CountryCode,
    /// Whether line prices include tax (the market decided, at checkout).
    pub taxes_included: bool,
    pub customer: CustomerId,
    /// The buyer's email at checkout; a later change on the customer doesn't rewrite it.
    pub email: Email,
    pub lines: Vec<OrderLine>,
    /// Sum of the line totals.
    pub subtotal: Money,
    /// Sum of the lines' tax: part of `subtotal` if `taxes_included`, otherwise on top.
    pub tax_total: Money,
    /// What the buyer pays.
    pub total: Money,
}

/// An order (design note 0007): a snapshot of a priced cart, plus its payment and
/// fulfillment status.
///
/// Invariants, checked by `place` and `restore`: at least one line, line IDs unique,
/// every amount in `currency`, each line's total is its unit price times its quantity,
/// and the totals add up from the lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    id: OrderId,
    version: Version,
    number: OrderNumber,
    placed_at: Timestamp,
    cart: CartId,
    market: MarketId,
    currency: Currency,
    country: CountryCode,
    taxes_included: bool,
    customer: CustomerId,
    email: Email,
    lines: Vec<OrderLine>,
    subtotal: Money,
    tax_total: Money,
    total: Money,
    financial_status: FinancialStatus,
    fulfillment_status: FulfillmentStatus,
}

impl Versioned for Order {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Order {
    /// A new order: payment pending, nothing fulfilled.
    pub fn place(new: NewOrder) -> Result<Order, OrderError> {
        Order::restore(OrderSnapshot {
            id: new.id,
            version: Version::NEW,
            number: new.number,
            placed_at: new.placed_at,
            cart: new.cart,
            market: new.market,
            currency: new.currency,
            country: new.country,
            taxes_included: new.taxes_included,
            customer: new.customer,
            email: new.email,
            lines: new.lines,
            subtotal: new.subtotal,
            tax_total: new.tax_total,
            total: new.total,
            financial_status: FinancialStatus::Pending,
            fulfillment_status: FulfillmentStatus::Unfulfilled,
        })
    }

    /// Records that the order is paid; returns whether that changed anything (an order
    /// already paid stays as it is).
    pub fn mark_paid(&mut self) -> bool {
        let changed = self.financial_status != FinancialStatus::Paid;
        self.financial_status = FinancialStatus::Paid;
        changed
    }

    pub fn id(&self) -> OrderId {
        self.id
    }

    pub fn number(&self) -> OrderNumber {
        self.number
    }

    pub fn placed_at(&self) -> Timestamp {
        self.placed_at
    }

    pub fn cart(&self) -> CartId {
        self.cart
    }

    pub fn market(&self) -> MarketId {
        self.market
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    pub fn country(&self) -> CountryCode {
        self.country
    }

    pub fn taxes_included(&self) -> bool {
        self.taxes_included
    }

    pub fn customer(&self) -> CustomerId {
        self.customer
    }

    pub fn email(&self) -> &Email {
        &self.email
    }

    pub fn lines(&self) -> &[OrderLine] {
        &self.lines
    }

    pub fn subtotal(&self) -> Money {
        self.subtotal
    }

    pub fn tax_total(&self) -> Money {
        self.tax_total
    }

    pub fn total(&self) -> Money {
        self.total
    }

    pub fn financial_status(&self) -> FinancialStatus {
        self.financial_status
    }

    pub fn fulfillment_status(&self) -> FulfillmentStatus {
        self.fulfillment_status
    }
}

/// Plain-data form of an `Order` for storage adapters (design note 0001, 3.4).
/// `restore` re-checks every invariant, so bad stored data fails loudly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderSnapshot {
    pub id: OrderId,
    pub version: Version,
    pub number: OrderNumber,
    pub placed_at: Timestamp,
    pub cart: CartId,
    pub market: MarketId,
    pub currency: Currency,
    pub country: CountryCode,
    pub taxes_included: bool,
    pub customer: CustomerId,
    pub email: Email,
    /// In display order.
    pub lines: Vec<OrderLine>,
    pub subtotal: Money,
    pub tax_total: Money,
    pub total: Money,
    pub financial_status: FinancialStatus,
    pub fulfillment_status: FulfillmentStatus,
}

impl Order {
    pub fn snapshot(&self) -> OrderSnapshot {
        OrderSnapshot {
            id: self.id,
            version: self.version,
            number: self.number,
            placed_at: self.placed_at,
            cart: self.cart,
            market: self.market,
            currency: self.currency,
            country: self.country,
            taxes_included: self.taxes_included,
            customer: self.customer,
            email: self.email.clone(),
            lines: self.lines.clone(),
            subtotal: self.subtotal,
            tax_total: self.tax_total,
            total: self.total,
            financial_status: self.financial_status,
            fulfillment_status: self.fulfillment_status,
        }
    }

    /// Rebuilds an order from storage (and builds new ones, through `place`), checking
    /// every invariant. Line text is trimmed like all free text.
    pub fn restore(mut s: OrderSnapshot) -> Result<Order, OrderError> {
        if s.lines.is_empty() {
            return Err(OrderError::NoLines);
        }
        let currency = s.currency;
        let in_currency = |m: Money| {
            if m.currency() == currency {
                Ok(())
            } else {
                Err(OrderError::CurrencyMismatch {
                    expected: currency,
                    found: m.currency(),
                })
            }
        };

        let mut seen = HashSet::new();
        let mut subtotal = Money::zero(currency);
        let mut tax_total = Money::zero(currency);
        for line in &mut s.lines {
            if !seen.insert(line.id) {
                return Err(OrderError::DuplicateLine(line.id));
            }
            line.product_title = required_text("product_title", &line.product_title)?;
            line.variant_title = copied_text(line.variant_title.take());
            line.sku = copied_text(line.sku.take());

            in_currency(line.unit_price.money())?;
            in_currency(line.total)?;
            if line.unit_price.money().checked_mul(line.quantity.value())? != line.total {
                return Err(OrderError::LineTotalMismatch(line.id));
            }
            subtotal = subtotal.checked_add(line.total)?;
            if let Some(tax) = &line.tax {
                in_currency(tax.amount)?;
                tax_total = tax_total.checked_add(tax.amount)?;
            }
        }

        for amount in [s.subtotal, s.tax_total, s.total] {
            in_currency(amount)?;
        }
        let total = if s.taxes_included {
            subtotal
        } else {
            subtotal.checked_add(tax_total)?
        };
        if (subtotal, tax_total, total) != (s.subtotal, s.tax_total, s.total) {
            return Err(OrderError::TotalsMismatch);
        }

        Ok(Order {
            id: s.id,
            version: s.version,
            number: s.number,
            placed_at: s.placed_at,
            cart: s.cart,
            market: s.market,
            currency: s.currency,
            country: s.country,
            taxes_included: s.taxes_included,
            customer: s.customer,
            email: s.email,
            lines: s.lines,
            subtotal: s.subtotal,
            tax_total: s.tax_total,
            total: s.total,
            financial_status: s.financial_status,
            fulfillment_status: s.fulfillment_status,
        })
    }
}

/// Text copied from a product that the product doesn't cap (option labels joined into a
/// variant title, a SKU): trimmed and blank as `None`, but no length limit. An order must
/// accept whatever the cart could sell, or checkout would fail on every retry.
fn copied_text(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    fn eur(amount: i64) -> Money {
        Money::new(amount, Currency::Eur).unwrap()
    }

    fn line_id(n: u128) -> OrderLineId {
        OrderLineId::from_uuid(Uuid::from_u128(n))
    }

    /// `quantity` × `unit` EUR, taxed `tax` EUR if given.
    fn line(n: u128, unit: i64, quantity: u32, tax: Option<i64>) -> OrderLine {
        OrderLine {
            id: line_id(n),
            product: ProductId::from_uuid(Uuid::from_u128(100 + n)),
            variant: VariantId::from_uuid(Uuid::from_u128(200 + n)),
            quantity: Quantity::new(quantity).unwrap(),
            product_title: format!("Product {n}"),
            variant_title: None,
            sku: None,
            unit_price: Price::new(eur(unit)).unwrap(),
            total: eur(unit * i64::from(quantity)),
            tax: tax.map(|amount| OrderTax {
                rate: TaxRate::new(250_000).unwrap(),
                class: None,
                amount: eur(amount),
                source: OrderTaxSource::RateTable {
                    region: TaxRegionId::from_uuid(Uuid::from_u128(9)),
                },
            }),
        }
    }

    /// Two lines: 2 × 10.00 taxed 5.00, and 1 × 4.00 untaxed; taxes on top.
    fn new_order() -> NewOrder {
        NewOrder {
            id: OrderId::from_uuid(Uuid::from_u128(1)),
            number: OrderNumber::new(1001).unwrap(),
            placed_at: Timestamp::from_unix_micros(1_767_225_600_000_000),
            cart: CartId::from_uuid(Uuid::from_u128(2)),
            market: MarketId::from_uuid(Uuid::from_u128(3)),
            currency: Currency::Eur,
            country: CountryCode::new("HR").unwrap(),
            taxes_included: false,
            customer: CustomerId::from_uuid(Uuid::from_u128(4)),
            email: Email::new("ana@example.hr").unwrap(),
            lines: vec![line(1, 1000, 2, Some(500)), line(2, 400, 1, None)],
            subtotal: eur(2400),
            tax_total: eur(500),
            total: eur(2900),
        }
    }

    #[test]
    fn a_placed_order_is_pending_and_unfulfilled() {
        let order = Order::place(new_order()).unwrap();
        assert_eq!(order.financial_status(), FinancialStatus::Pending);
        assert_eq!(order.fulfillment_status(), FulfillmentStatus::Unfulfilled);
        assert_eq!(order.version(), Version::NEW);
    }

    #[test]
    fn an_order_needs_a_line() {
        let mut new = new_order();
        new.lines.clear();
        assert_eq!(Order::place(new), Err(OrderError::NoLines));
    }

    #[test]
    fn line_ids_are_unique() {
        let mut new = new_order();
        new.lines[1].id = line_id(1);
        assert_eq!(
            Order::place(new),
            Err(OrderError::DuplicateLine(line_id(1)))
        );
    }

    #[test]
    fn every_amount_is_in_the_order_currency() {
        let mut new = new_order();
        new.lines[1].unit_price = Price::new(Money::new(400, Currency::Usd).unwrap()).unwrap();
        assert_eq!(
            Order::place(new),
            Err(OrderError::CurrencyMismatch {
                expected: Currency::Eur,
                found: Currency::Usd
            })
        );
    }

    #[test]
    fn a_line_total_is_unit_price_times_quantity() {
        let mut new = new_order();
        new.lines[0].total = eur(1999);
        assert_eq!(
            Order::place(new),
            Err(OrderError::LineTotalMismatch(line_id(1)))
        );
    }

    #[test]
    fn totals_add_up_from_the_lines() {
        let mut new = new_order();
        new.total = eur(2400);
        assert_eq!(Order::place(new), Err(OrderError::TotalsMismatch));
    }

    #[test]
    fn included_tax_is_not_added_to_the_total() {
        let mut new = new_order();
        new.taxes_included = true;
        new.total = eur(2400);
        assert!(Order::place(new).is_ok());
    }

    #[test]
    fn line_text_is_trimmed_a_title_is_required_and_copied_text_is_uncapped() {
        let mut new = new_order();
        new.lines[0].product_title = " Mug ".to_string();
        new.lines[0].sku = Some("  ".to_string());
        let order = Order::place(new).unwrap();
        assert_eq!(order.lines()[0].product_title, "Mug");
        assert_eq!(order.lines()[0].sku, None);

        // A variant title joins one label per option, so it can be longer than any
        // single field; the order keeps it whole.
        let long = "S / ".repeat(100);
        let mut long_title = new_order();
        long_title.lines[0].variant_title = Some(long.clone());
        long_title.lines[0].sku = Some("x".repeat(300));
        let order = Order::place(long_title).unwrap();
        assert_eq!(order.lines()[0].variant_title.as_deref(), Some(long.trim()));
        assert_eq!(order.lines()[0].sku.as_deref().map(str::len), Some(300));

        let mut untitled = new_order();
        untitled.lines[0].product_title = " ".to_string();
        assert_eq!(
            Order::place(untitled),
            Err(OrderError::Text(TextError::Missing("product_title")))
        );
    }

    #[test]
    fn mark_paid_changes_a_pending_order_once() {
        let mut order = Order::place(new_order()).unwrap();
        assert!(order.mark_paid());
        assert_eq!(order.financial_status(), FinancialStatus::Paid);
        assert!(!order.mark_paid());
    }

    #[test]
    fn restore_round_trips() {
        let mut order = Order::place(new_order()).unwrap();
        order.mark_paid();
        assert_eq!(Order::restore(order.snapshot()), Ok(order));
    }

    #[test]
    fn order_numbers_start_at_1() {
        assert_eq!(OrderNumber::new(0), Err(OrderError::InvalidNumber(0)));
        assert_eq!(OrderNumber::new(1).map(OrderNumber::value), Ok(1));
        assert_eq!(OrderNumber::new(1001).unwrap().to_string(), "#1001");
    }

    #[test]
    fn statuses_parse_back_from_their_strings() {
        for status in [FinancialStatus::Pending, FinancialStatus::Paid] {
            assert_eq!(status.as_str().parse(), Ok(status));
        }
        assert_eq!("unfulfilled".parse(), Ok(FulfillmentStatus::Unfulfilled));
        assert_eq!(
            "refunded".parse::<FinancialStatus>(),
            Err("refunded".to_string())
        );
    }
}
