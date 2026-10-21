use std::{collections::HashSet, fmt, str::FromStr};

use hypha_core::{
    Address, CartId, ChannelId, CountryCode, Currency, CustomerId, Email, MarketId, Money, OrderId,
    OrderLineId, PendingEvent, Price, ProductId, Quantity, Recorded, Recorder, ShippingGroupId,
    ShippingOptionId, Stamped, Stamps, Subject, TaxClassId, TaxRate, TaxRegionId, Timestamp,
    VariantId, Version, Versioned, required_text,
};

use crate::{
    OrderError,
    events::{ORDER, OrderCancelled, OrderFulfillmentStatusChanged, OrderPaid, OrderPlaced},
};

/// An order's human number (`#1001`) from `OrderRepo::next_number`: unique and
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

/// Where the money is (design note 0027): derived from the order's payments and
/// stored here; defined in core so `hypha-payments` can derive it without depending on
/// orders.
pub use hypha_core::FinancialStatus;

/// Where the goods are (design note 0007, O3), derived from the lines' fulfilled
/// quantities (design note 0018, FU3) and stored for lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FulfillmentStatus {
    Unfulfilled,
    PartiallyFulfilled,
    Fulfilled,
}

impl FulfillmentStatus {
    /// The same string used in JSON and storage, e.g. "unfulfilled".
    pub fn as_str(&self) -> &'static str {
        match self {
            FulfillmentStatus::Unfulfilled => "unfulfilled",
            FulfillmentStatus::PartiallyFulfilled => "partially_fulfilled",
            FulfillmentStatus::Fulfilled => "fulfilled",
        }
    }

    /// The status of `lines`: nothing fulfilled, all of every line, or anything between.
    pub fn of(lines: &[OrderLine]) -> FulfillmentStatus {
        if lines.iter().all(|l| l.fulfilled_quantity == 0) {
            FulfillmentStatus::Unfulfilled
        } else if lines
            .iter()
            .all(|l| l.fulfilled_quantity == l.quantity.value())
        {
            FulfillmentStatus::Fulfilled
        } else {
            FulfillmentStatus::PartiallyFulfilled
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
            "partially_fulfilled" => Ok(FulfillmentStatus::PartiallyFulfilled),
            "fulfilled" => Ok(FulfillmentStatus::Fulfilled),
            other => Err(other.to_string()),
        }
    }
}

/// Why an order was cancelled (design note 0018, FU7; Shopify's reasons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// The buyer asked.
    Customer,
    /// The goods ran out or can't be shipped.
    Inventory,
    Fraud,
    /// Payment was declined.
    Declined,
    /// Staff cancelled it for a reason of their own.
    Staff,
    Other,
}

impl CancelReason {
    /// The same string used in JSON and storage, e.g. "customer".
    pub fn as_str(&self) -> &'static str {
        match self {
            CancelReason::Customer => "customer",
            CancelReason::Inventory => "inventory",
            CancelReason::Fraud => "fraud",
            CancelReason::Declined => "declined",
            CancelReason::Staff => "staff",
            CancelReason::Other => "other",
        }
    }
}

impl fmt::Display for CancelReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CancelReason {
    /// The text that didn't match a reason.
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "customer" => Ok(CancelReason::Customer),
            "inventory" => Ok(CancelReason::Inventory),
            "fraud" => Ok(CancelReason::Fraud),
            "declined" => Ok(CancelReason::Declined),
            "staff" => Ok(CancelReason::Staff),
            "other" => Ok(CancelReason::Other),
            other => Err(other.to_string()),
        }
    }
}

/// When and why an order was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancellation {
    pub at: Timestamp,
    pub reason: CancelReason,
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
    /// How many of `quantity` standing fulfilments shipped (design note 0018, FU3): 0
    /// at checkout, never more than `quantity`.
    pub fulfilled_quantity: u32,
}

impl OrderLine {
    /// How many are still to ship.
    pub fn unfulfilled_quantity(&self) -> u32 {
        self.quantity
            .value()
            .saturating_sub(self.fulfilled_quantity)
    }
}

/// How one group of the order ships, as it was charged (design note 0015, SH12): copies,
/// like lines, so the order reads the same after the option or its provider changes.
/// Plain values, so orders don't depend on the shipping crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderShippingLine {
    pub group: ShippingGroupId,
    /// The order lines this ships: what fulfilment (0018) works through.
    pub lines: Vec<OrderLineId>,
    pub option: ShippingOptionId,
    pub code: String,
    pub name: String,
    /// The provider that quoted it (`app_…`, `spore_…`) and its service; `None` for a
    /// built-in rate.
    pub provider: Option<String>,
    pub service: Option<String>,
    /// The provider's quote ID: what fulfilment buys the label with (0018, section 8).
    pub reference: Option<String>,
    pub amount: Price,
    /// Delivery in days, `min` to `max`, as estimated at checkout.
    pub estimate_days: Option<(u16, u16)>,
    /// `None` when the country had no tax rates.
    pub tax: Option<OrderTax>,
}

/// What checkout knows when it places an order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOrder {
    pub id: OrderId,
    pub number: OrderNumber,
    pub placed_at: Timestamp,
    pub cart: CartId,
    /// The sales channel it was placed in (design note 0020, CH7).
    pub channel: ChannelId,
    pub market: MarketId,
    pub currency: Currency,
    pub country: CountryCode,
    /// Whether line prices include tax (the market decided, at checkout).
    pub taxes_included: bool,
    pub customer: CustomerId,
    /// The buyer's email at checkout; a later change on the customer doesn't rewrite it.
    pub email: Email,
    pub lines: Vec<OrderLine>,
    /// Where it ships; required with shipping lines.
    pub shipping_address: Option<Address>,
    pub shipping_lines: Vec<OrderShippingLine>,
    /// Sum of the line totals.
    pub subtotal: Money,
    /// Sum of the shipping lines' amounts.
    pub shipping_total: Money,
    /// Tax on the lines and on shipping: part of `subtotal` and `shipping_total` if
    /// `taxes_included`, otherwise on top.
    pub tax_total: Money,
    /// What the buyer pays.
    pub total: Money,
}

/// An order (design note 0007): a snapshot of a priced cart, plus its payment and
/// fulfillment status, and its cancellation (design note 0018).
///
/// Invariants, checked by `place` and `restore`: at least one line, line IDs unique,
/// every amount in `currency`, each line's total is its unit price times its quantity,
/// the totals add up from the lines, no line fulfilled past its quantity, and the
/// fulfillment status is the lines'.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    id: OrderId,
    version: Version,
    /// When it was stored and last changed, set by the adapter (design note 0016).
    stamps: Option<Stamps>,
    number: OrderNumber,
    placed_at: Timestamp,
    cart: CartId,
    /// The sales channel it was placed in (design note 0020, CH7).
    channel: ChannelId,
    market: MarketId,
    currency: Currency,
    country: CountryCode,
    taxes_included: bool,
    customer: CustomerId,
    email: Email,
    lines: Vec<OrderLine>,
    shipping_address: Option<Address>,
    shipping_lines: Vec<OrderShippingLine>,
    subtotal: Money,
    shipping_total: Money,
    tax_total: Money,
    total: Money,
    financial_status: FinancialStatus,
    fulfillment_status: FulfillmentStatus,
    cancelled: Option<Cancellation>,
    events: Recorder,
}

impl Recorded for Order {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for Order {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Order {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl Order {
    /// A new order: payment pending, nothing fulfilled.
    pub fn place(new: NewOrder) -> Result<Order, OrderError> {
        let mut order = Order::restore(OrderSnapshot {
            id: new.id,
            version: Version::NEW,
            stamps: None,
            number: new.number,
            placed_at: new.placed_at,
            cart: new.cart,
            channel: new.channel,
            market: new.market,
            currency: new.currency,
            country: new.country,
            taxes_included: new.taxes_included,
            customer: new.customer,
            email: new.email,
            lines: new.lines,
            shipping_address: new.shipping_address,
            shipping_lines: new.shipping_lines,
            subtotal: new.subtotal,
            shipping_total: new.shipping_total,
            tax_total: new.tax_total,
            total: new.total,
            financial_status: FinancialStatus::Pending,
            // Lines that say otherwise fail `restore`'s status check.
            fulfillment_status: FulfillmentStatus::Unfulfilled,
            cancelled: None,
        })?;
        let placed = OrderPlaced {
            order: order.id,
            number: order.number.value(),
            customer: order.customer,
            channel: order.channel,
            total: order.total,
        };
        order.events.record(order.subject(), &placed);
        Ok(order)
    }

    fn subject(&self) -> Subject {
        Subject::new(ORDER, self.id.as_uuid())
    }

    /// Sets the payment status its payments now add up to (design note 0027, PA3), and
    /// returns whether it changed. Becoming `paid` records `order.paid`. Not refused for
    /// a cancelled order: cancelling voids its pending payments, which moves it too. The
    /// workflow that changes a payment calls this in the same batch.
    pub fn set_financial_status(&mut self, status: FinancialStatus) -> bool {
        if self.financial_status == status {
            return false;
        }
        self.financial_status = status;
        if status == FinancialStatus::Paid {
            let paid = OrderPaid {
                order: self.id,
                amount: self.total,
            };
            self.events.record(self.subject(), &paid);
        }
        true
    }

    /// Refused for a cancelled order (`order_cancelled`): what shipping it and recording
    /// a payment by hand check first.
    pub fn refuse_if_cancelled(&self) -> Result<(), OrderError> {
        match self.cancelled {
            Some(_) => Err(OrderError::Cancelled(self.id)),
            None => Ok(()),
        }
    }

    /// Counts `lines` (line, quantity) as shipped by a new fulfilment. All or nothing:
    /// refused for a cancelled order, a line the order doesn't have, or more than a
    /// line has left to ship (the same line twice counts twice).
    pub fn fulfil(&mut self, lines: &[(OrderLineId, Quantity)]) -> Result<(), OrderError> {
        self.refuse_if_cancelled()?;
        let mut next = self.lines.clone();
        for (id, quantity) in lines {
            let line = line_in(&mut next, *id)?;
            let requested = quantity.value();
            if requested > line.unfulfilled_quantity() {
                return Err(OrderError::OverFulfilled {
                    line: *id,
                    requested,
                    unfulfilled: line.unfulfilled_quantity(),
                });
            }
            line.fulfilled_quantity += requested;
        }
        self.set_lines(next);
        Ok(())
    }

    /// Takes back `lines` (line, quantity) of a cancelled fulfilment. All or nothing:
    /// refused for a cancelled order (its stock is gone back already), a line the order
    /// doesn't have, or more than a line has fulfilled.
    pub fn unfulfil(&mut self, lines: &[(OrderLineId, Quantity)]) -> Result<(), OrderError> {
        self.refuse_if_cancelled()?;
        let mut next = self.lines.clone();
        for (id, quantity) in lines {
            let line = line_in(&mut next, *id)?;
            let requested = quantity.value();
            if requested > line.fulfilled_quantity {
                return Err(OrderError::NotFulfilled {
                    line: *id,
                    requested,
                    fulfilled: line.fulfilled_quantity,
                });
            }
            line.fulfilled_quantity -= requested;
        }
        self.set_lines(next);
        Ok(())
    }

    /// Replaces the lines (only their fulfilled quantities differ) and records the
    /// status change, if any.
    fn set_lines(&mut self, lines: Vec<OrderLine>) {
        let from = self.fulfillment_status;
        self.lines = lines;
        self.fulfillment_status = FulfillmentStatus::of(&self.lines);
        if self.fulfillment_status != from {
            let changed = OrderFulfillmentStatusChanged {
                order: self.id,
                from: from.as_str(),
                to: self.fulfillment_status.as_str(),
            };
            self.events.record(self.subject(), &changed);
        }
    }

    /// Cancels the order; returns whether that changed anything (a cancelled order
    /// stays as it was, with its first reason). Refused while anything is fulfilled:
    /// those fulfilments are cancelled first. Payment is untouched (FU7).
    /// `notify_customer` travels in `order.cancelled`: whether the buyer is emailed
    /// (design note 0028, NT7).
    pub fn cancel(
        &mut self,
        at: Timestamp,
        reason: CancelReason,
        notify_customer: bool,
    ) -> Result<bool, OrderError> {
        if self.cancelled.is_some() {
            return Ok(false);
        }
        if self.fulfillment_status != FulfillmentStatus::Unfulfilled {
            return Err(OrderError::HasFulfillments(self.id));
        }
        self.cancelled = Some(Cancellation { at, reason });
        let cancelled = OrderCancelled {
            order: self.id,
            reason: reason.as_str(),
            notify_customer,
        };
        self.events.record(self.subject(), &cancelled);
        Ok(true)
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

    /// The sales channel it was placed in.
    pub fn channel(&self) -> ChannelId {
        self.channel
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

    /// Erasure (design note 0032, §7.3): the buyer's email becomes `placeholder` and the
    /// shipping address goes (the order's country stays). Lines, totals, payments and
    /// fulfilments are untouched: placed orders are never deleted. Records no event of its
    /// own: `customer.redacted` names the orders.
    pub fn redact(&mut self, placeholder: Email) -> bool {
        let mut changed = false;
        if self.email != placeholder {
            self.email = placeholder;
            changed = true;
        }
        if self.shipping_address.take().is_some() {
            changed = true;
        }
        changed
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

    pub fn shipping_address(&self) -> Option<&Address> {
        self.shipping_address.as_ref()
    }

    pub fn shipping_lines(&self) -> &[OrderShippingLine] {
        &self.shipping_lines
    }

    pub fn shipping_total(&self) -> Money {
        self.shipping_total
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

    pub fn cancelled(&self) -> Option<Cancellation> {
        self.cancelled
    }
}

/// Plain-data form of an `Order` for storage adapters (design note 0001, 3.4).
/// `restore` re-checks every invariant, so bad stored data fails loudly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderSnapshot {
    pub id: OrderId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub number: OrderNumber,
    pub placed_at: Timestamp,
    pub cart: CartId,
    /// The sales channel it was placed in (design note 0020, CH7).
    pub channel: ChannelId,
    pub market: MarketId,
    pub currency: Currency,
    pub country: CountryCode,
    pub taxes_included: bool,
    pub customer: CustomerId,
    pub email: Email,
    /// In display order.
    pub lines: Vec<OrderLine>,
    pub shipping_address: Option<Address>,
    pub shipping_lines: Vec<OrderShippingLine>,
    pub subtotal: Money,
    pub shipping_total: Money,
    pub tax_total: Money,
    pub total: Money,
    pub financial_status: FinancialStatus,
    pub fulfillment_status: FulfillmentStatus,
    pub cancelled: Option<Cancellation>,
}

impl Order {
    pub fn snapshot(&self) -> OrderSnapshot {
        OrderSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            number: self.number,
            placed_at: self.placed_at,
            cart: self.cart,
            channel: self.channel,
            market: self.market,
            currency: self.currency,
            country: self.country,
            taxes_included: self.taxes_included,
            customer: self.customer,
            email: self.email.clone(),
            lines: self.lines.clone(),
            shipping_address: self.shipping_address.clone(),
            shipping_lines: self.shipping_lines.clone(),
            subtotal: self.subtotal,
            shipping_total: self.shipping_total,
            tax_total: self.tax_total,
            total: self.total,
            financial_status: self.financial_status,
            fulfillment_status: self.fulfillment_status,
            cancelled: self.cancelled,
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

            if line.fulfilled_quantity > line.quantity.value() {
                return Err(OrderError::FulfilledPastQuantity(line.id));
            }
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

        if !s.shipping_lines.is_empty() && s.shipping_address.is_none() {
            return Err(OrderError::ShippingWithoutAddress);
        }
        let mut shipping_total = Money::zero(currency);
        for line in &mut s.shipping_lines {
            if let Some(unknown) = line.lines.iter().find(|id| !seen.contains(*id)) {
                return Err(OrderError::UnknownShippedLine(*unknown));
            }
            line.name = required_text("shipping name", &line.name)?;
            in_currency(line.amount.money())?;
            shipping_total = shipping_total.checked_add(line.amount.money())?;
            if let Some(tax) = &line.tax {
                in_currency(tax.amount)?;
                tax_total = tax_total.checked_add(tax.amount)?;
            }
        }

        for amount in [s.subtotal, s.shipping_total, s.tax_total, s.total] {
            in_currency(amount)?;
        }
        let gross = subtotal.checked_add(shipping_total)?;
        let total = if s.taxes_included {
            gross
        } else {
            gross.checked_add(tax_total)?
        };
        if (subtotal, shipping_total, tax_total, total)
            != (s.subtotal, s.shipping_total, s.tax_total, s.total)
        {
            return Err(OrderError::TotalsMismatch);
        }

        if FulfillmentStatus::of(&s.lines) != s.fulfillment_status {
            return Err(OrderError::FulfillmentStatusMismatch);
        }

        Ok(Order {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            number: s.number,
            placed_at: s.placed_at,
            cart: s.cart,
            channel: s.channel,
            market: s.market,
            currency: s.currency,
            country: s.country,
            taxes_included: s.taxes_included,
            customer: s.customer,
            email: s.email,
            lines: s.lines,
            shipping_address: s.shipping_address,
            shipping_lines: s.shipping_lines,
            subtotal: s.subtotal,
            shipping_total: s.shipping_total,
            tax_total: s.tax_total,
            total: s.total,
            financial_status: s.financial_status,
            fulfillment_status: s.fulfillment_status,
            cancelled: s.cancelled,
            events: Recorder::default(),
        })
    }
}

fn line_in(lines: &mut [OrderLine], id: OrderLineId) -> Result<&mut OrderLine, OrderError> {
    lines
        .iter_mut()
        .find(|l| l.id == id)
        .ok_or(OrderError::UnknownLine(id))
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
        Money::new(amount, Currency::EUR).unwrap()
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
            fulfilled_quantity: 0,
        }
    }

    /// Two lines: 2 × 10.00 taxed 5.00, and 1 × 4.00 untaxed; taxes on top.
    fn shipping_line(amount: i64, tax: Option<i64>) -> OrderShippingLine {
        OrderShippingLine {
            group: ShippingGroupId::from_uuid(Uuid::from_u128(50)),
            lines: vec![line_id(1)],
            option: ShippingOptionId::from_uuid(Uuid::from_u128(51)),
            code: "standard".to_string(),
            name: " Standard ".to_string(),
            provider: None,
            service: None,
            reference: None,
            amount: Price::new(eur(amount)).unwrap(),
            estimate_days: Some((1, 3)),
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

    fn zagreb() -> Address {
        Address::new(hypha_core::AddressParts {
            first_name: None,
            last_name: "Horvat".to_string(),
            company: None,
            line1: "Ilica 1".to_string(),
            line2: None,
            city: "Zagreb".to_string(),
            province: None,
            postal_code: None,
            country: "HR".to_string(),
            phone: None,
        })
        .unwrap()
    }

    #[test]
    fn shipping_counts_in_the_totals_and_its_tax_in_the_tax_total() {
        let order = Order::place(NewOrder {
            shipping_address: Some(zagreb()),
            shipping_lines: vec![shipping_line(400, Some(100))],
            shipping_total: eur(400),
            tax_total: eur(600),
            total: eur(3400),
            ..new_order()
        })
        .unwrap();
        assert_eq!(order.shipping_lines()[0].name, "Standard");
        assert_eq!(
            (order.shipping_total(), order.total()),
            (eur(400), eur(3400))
        );
    }

    #[test]
    fn totals_that_leave_out_shipping_dont_add_up() {
        let result = Order::place(NewOrder {
            shipping_address: Some(zagreb()),
            shipping_lines: vec![shipping_line(400, None)],
            ..new_order()
        });
        assert_eq!(result, Err(OrderError::TotalsMismatch));
    }

    #[test]
    fn shipping_lines_need_an_address() {
        let result = Order::place(NewOrder {
            shipping_lines: vec![shipping_line(400, None)],
            shipping_total: eur(400),
            total: eur(3300),
            ..new_order()
        });
        assert_eq!(result, Err(OrderError::ShippingWithoutAddress));
    }

    fn new_order() -> NewOrder {
        NewOrder {
            id: OrderId::from_uuid(Uuid::from_u128(1)),
            number: OrderNumber::new(1001).unwrap(),
            placed_at: Timestamp::from_unix_micros(1_767_225_600_000_000),
            cart: CartId::from_uuid(Uuid::from_u128(2)),
            channel: ChannelId::from_uuid(Uuid::from_u128(500)),
            market: MarketId::from_uuid(Uuid::from_u128(3)),
            currency: Currency::EUR,
            country: CountryCode::new("HR").unwrap(),
            taxes_included: false,
            customer: CustomerId::from_uuid(Uuid::from_u128(4)),
            email: Email::new("ana@example.hr").unwrap(),
            lines: vec![line(1, 1000, 2, Some(500)), line(2, 400, 1, None)],
            shipping_address: None,
            shipping_lines: vec![],
            subtotal: eur(2400),
            shipping_total: eur(0),
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

    // ---- fulfilment and cancellation (design note 0018) ----

    fn q(n: u32) -> Quantity {
        Quantity::new(n).unwrap()
    }

    fn placed() -> Order {
        let mut order = Order::place(new_order()).unwrap();
        order.take_events();
        order
    }

    fn event_types(order: &mut Order) -> Vec<&'static str> {
        order.take_events().iter().map(|e| e.event_type).collect()
    }

    fn at() -> Timestamp {
        Timestamp::from_unix_micros(1_767_312_000_000_000)
    }

    #[test]
    fn fulfilling_part_of_an_order_makes_it_partially_fulfilled() {
        let mut order = placed();
        order.fulfil(&[(line_id(1), q(1))]).unwrap();
        assert_eq!(order.lines()[0].fulfilled_quantity, 1);
        assert_eq!(order.lines()[0].unfulfilled_quantity(), 1);
        assert_eq!(
            order.fulfillment_status(),
            FulfillmentStatus::PartiallyFulfilled
        );
        assert_eq!(
            event_types(&mut order),
            ["order.fulfillment_status_changed"]
        );
    }

    #[test]
    fn fulfilling_every_line_makes_it_fulfilled() {
        let mut order = placed();
        order.fulfil(&[(line_id(1), q(1))]).unwrap();
        order.take_events();
        order
            .fulfil(&[(line_id(1), q(1)), (line_id(2), q(1))])
            .unwrap();
        assert_eq!(order.fulfillment_status(), FulfillmentStatus::Fulfilled);
        assert_eq!(
            event_types(&mut order),
            ["order.fulfillment_status_changed"]
        );
    }

    #[test]
    fn a_fulfilment_that_keeps_the_status_records_no_event() {
        let mut order = Order::place(NewOrder {
            lines: vec![line(1, 1000, 3, None)],
            subtotal: eur(3000),
            tax_total: eur(0),
            total: eur(3000),
            ..new_order()
        })
        .unwrap();
        order.take_events();
        order.fulfil(&[(line_id(1), q(1))]).unwrap();
        order.take_events();
        order.fulfil(&[(line_id(1), q(1))]).unwrap();
        assert_eq!(event_types(&mut order), Vec::<&str>::new());
    }

    #[test]
    fn a_line_cant_be_fulfilled_past_its_quantity() {
        let mut order = placed();
        let before = order.clone();
        assert_eq!(
            order.fulfil(&[(line_id(2), q(1)), (line_id(1), q(3))]),
            Err(OrderError::OverFulfilled {
                line: line_id(1),
                requested: 3,
                unfulfilled: 2
            })
        );
        assert_eq!(order, before);
    }

    #[test]
    fn the_same_line_twice_counts_twice() {
        let mut order = placed();
        assert_eq!(
            order.fulfil(&[(line_id(1), q(2)), (line_id(1), q(1))]),
            Err(OrderError::OverFulfilled {
                line: line_id(1),
                requested: 1,
                unfulfilled: 0
            })
        );
    }

    #[test]
    fn fulfilling_a_line_the_order_doesnt_have_is_refused() {
        let mut order = placed();
        assert_eq!(
            order.fulfil(&[(line_id(9), q(1))]),
            Err(OrderError::UnknownLine(line_id(9)))
        );
    }

    #[test]
    fn unfulfilling_takes_the_count_back() {
        let mut order = placed();
        order.fulfil(&[(line_id(1), q(2))]).unwrap();
        order.take_events();
        order.unfulfil(&[(line_id(1), q(2))]).unwrap();
        assert_eq!(order.fulfillment_status(), FulfillmentStatus::Unfulfilled);
        assert_eq!(
            event_types(&mut order),
            ["order.fulfillment_status_changed"]
        );
    }

    #[test]
    fn unfulfilling_more_than_was_fulfilled_is_refused() {
        let mut order = placed();
        order.fulfil(&[(line_id(1), q(1))]).unwrap();
        let before = order.clone();
        assert_eq!(
            order.unfulfil(&[(line_id(1), q(2))]),
            Err(OrderError::NotFulfilled {
                line: line_id(1),
                requested: 2,
                fulfilled: 1
            })
        );
        assert_eq!(order, before);
    }

    #[test]
    fn cancelling_records_when_and_why() {
        let mut order = placed();
        assert_eq!(order.cancel(at(), CancelReason::Customer, false), Ok(true));
        assert_eq!(
            order.cancelled(),
            Some(Cancellation {
                at: at(),
                reason: CancelReason::Customer
            })
        );
        assert_eq!(event_types(&mut order), ["order.cancelled"]);
    }

    #[test]
    fn cancelling_again_keeps_the_first_reason() {
        let mut order = placed();
        order.cancel(at(), CancelReason::Customer, false).unwrap();
        order.take_events();
        assert_eq!(order.cancel(at(), CancelReason::Fraud, false), Ok(false));
        assert_eq!(
            order.cancelled().map(|c| c.reason),
            Some(CancelReason::Customer)
        );
        assert_eq!(event_types(&mut order), Vec::<&str>::new());
    }

    #[test]
    fn an_order_with_fulfilments_cant_be_cancelled() {
        let mut order = placed();
        order.fulfil(&[(line_id(2), q(1))]).unwrap();
        assert_eq!(
            order.cancel(at(), CancelReason::Staff, false),
            Err(OrderError::HasFulfillments(order.id()))
        );
        assert_eq!(order.cancelled(), None);
    }

    #[test]
    fn a_cancelled_order_cant_be_fulfilled_unfulfilled_or_paid_by_hand() {
        let mut order = placed();
        order.cancel(at(), CancelReason::Inventory, false).unwrap();
        let cancelled = Err(OrderError::Cancelled(order.id()));
        assert_eq!(order.fulfil(&[(line_id(1), q(1))]), cancelled);
        assert_eq!(order.unfulfil(&[(line_id(1), q(1))]), cancelled);
        assert_eq!(order.refuse_if_cancelled(), cancelled);
    }

    #[test]
    fn a_paid_order_can_be_cancelled_and_stays_paid() {
        // Refunds come with payment spores (FU7, Q3).
        let mut order = placed();
        order.set_financial_status(FinancialStatus::Paid);
        assert_eq!(order.cancel(at(), CancelReason::Customer, false), Ok(true));
        assert_eq!(order.financial_status(), FinancialStatus::Paid);
        assert!(!order.set_financial_status(FinancialStatus::Paid));
    }

    #[test]
    fn restore_rejects_a_line_fulfilled_past_its_quantity() {
        let mut s = placed().snapshot();
        s.lines[1].fulfilled_quantity = 2;
        s.fulfillment_status = FulfillmentStatus::PartiallyFulfilled;
        assert_eq!(
            Order::restore(s),
            Err(OrderError::FulfilledPastQuantity(line_id(2)))
        );
    }

    #[test]
    fn restore_rejects_a_status_the_lines_dont_have() {
        let mut s = placed().snapshot();
        s.fulfillment_status = FulfillmentStatus::Fulfilled;
        assert_eq!(
            Order::restore(s),
            Err(OrderError::FulfillmentStatusMismatch)
        );
    }

    #[test]
    fn statuses_and_reasons_read_back_from_their_strings() {
        for status in [
            FulfillmentStatus::Unfulfilled,
            FulfillmentStatus::PartiallyFulfilled,
            FulfillmentStatus::Fulfilled,
        ] {
            assert_eq!(status.as_str().parse(), Ok(status));
        }
        for reason in [
            CancelReason::Customer,
            CancelReason::Inventory,
            CancelReason::Fraud,
            CancelReason::Declined,
            CancelReason::Staff,
            CancelReason::Other,
        ] {
            assert_eq!(reason.as_str().parse(), Ok(reason));
        }
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
        new.lines[1].unit_price = Price::new(Money::new(400, Currency::USD).unwrap()).unwrap();
        assert_eq!(
            Order::place(new),
            Err(OrderError::CurrencyMismatch {
                expected: Currency::EUR,
                found: Currency::USD
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
    fn becoming_paid_records_order_paid_once() {
        let mut order = Order::place(new_order()).unwrap();
        order.take_events();
        assert!(order.set_financial_status(FinancialStatus::Paid));
        assert_eq!(order.financial_status(), FinancialStatus::Paid);
        assert!(!order.set_financial_status(FinancialStatus::Paid));
        let events = order.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "order.paid");
    }

    #[test]
    fn other_statuses_record_nothing() {
        let mut order = Order::place(new_order()).unwrap();
        order.take_events();
        assert!(order.set_financial_status(FinancialStatus::Voided));
        assert!(order.set_financial_status(FinancialStatus::PartiallyPaid));
        assert_eq!(order.take_events().len(), 0);
    }

    #[test]
    fn restore_round_trips() {
        let mut order = Order::place(new_order()).unwrap();
        order.set_financial_status(FinancialStatus::Paid);
        order.fulfil(&[(line_id(2), q(1))]).unwrap();
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
        assert_eq!("unfulfilled".parse(), Ok(FulfillmentStatus::Unfulfilled));
        assert_eq!(
            "shipped".parse::<FulfillmentStatus>(),
            Err("shipped".to_string())
        );
    }
}
