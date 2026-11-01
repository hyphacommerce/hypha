//! A purchase order: what the store orders from one supplier, to one of its locations,
//! and how much of it has arrived.

use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use hypha_core::{
    Currency, Email, ExchangeRate, LocationId, Money, PendingEvent, PurchaseOrderId, Recorded,
    Stamped, Stamps, Subject, SupplierId, Timestamp, UnitCost, VariantId, Version, Versioned,
    optional_text, required_text,
};

use crate::{
    CostAllocation, Outstanding, PaymentTerms, PaymentTermsDraft, PurchasingError,
    costs::{CostShare, LineBasis, extra_per_unit, landed_unit_cost},
    events::{
        PURCHASE_ORDER, PurchaseOrderCancelled, PurchaseOrderClosed, PurchaseOrderCreated,
        PurchaseOrderOrdered, PurchaseOrderReceiptStatusChanged, PurchaseOrderSent,
        PurchaseOrderUpdated, ShipTo,
    },
    supplier::MAX_NOTES_CHARS,
};

/// The most lines an order holds, and the most extra costs.
pub const MAX_LINES: usize = 1000;
pub const MAX_COSTS: usize = 20;
/// The most units a line orders, or counts as received or rejected: storage keeps the
/// counts as 32-bit integers, and no order comes near it.
pub const MAX_QUANTITY: u32 = 1_000_000_000;
/// The most addresses one send goes to, `to` and `cc` together, and the longest message.
pub const MAX_RECIPIENTS: usize = 10;
pub const MAX_MESSAGE_CHARS: usize = 2000;

/// One send of an order to its supplier: who gets it, what's said with it, where the
/// goods go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sending {
    pub to: Vec<Email>,
    pub cc: Vec<Email>,
    pub message: Option<String>,
    pub ship_to: Option<ShipTo>,
}

impl Sending {
    /// Repeats dropped (a `cc` already in `to` too), the message trimmed (blank is none),
    /// each rule checked: at least one `to`, at most `MAX_RECIPIENTS` in all, at most
    /// `MAX_MESSAGE_CHARS` of message.
    fn checked(self) -> Result<Self, PurchasingError> {
        let mut seen = HashSet::new();
        let to: Vec<Email> = self
            .to
            .into_iter()
            .filter(|e| seen.insert(e.clone()))
            .collect();
        let cc: Vec<Email> = self
            .cc
            .into_iter()
            .filter(|e| seen.insert(e.clone()))
            .collect();
        if to.is_empty() {
            return Err(PurchasingError::NoRecipients);
        }
        if to.len() + cc.len() > MAX_RECIPIENTS {
            return Err(PurchasingError::TooManyRecipients {
                max: MAX_RECIPIENTS,
            });
        }
        let message = self
            .message
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty());
        if message
            .as_ref()
            .is_some_and(|m| m.chars().count() > MAX_MESSAGE_CHARS)
        {
            return Err(PurchasingError::MessageTooLong {
                max: MAX_MESSAGE_CHARS,
            });
        }
        Ok(Self {
            to,
            cc,
            message,
            ship_to: self.ship_to,
        })
    }
}

/// A purchase order's human number (`PO-1001`): unique and increasing, possibly with gaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PurchaseOrderNumber(i64);

impl PurchaseOrderNumber {
    pub fn new(n: i64) -> Result<Self, PurchasingError> {
        if n < 1 {
            return Err(PurchasingError::InvalidNumber(n));
        }
        Ok(Self(n))
    }

    pub fn value(self) -> i64 {
        self.0
    }
}

impl fmt::Display for PurchaseOrderNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PO-{}", self.0)
    }
}

/// Where a purchase order is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PurchaseOrderStatus {
    /// Being written: anything may change, and it may be deleted.
    #[default]
    Draft,
    /// Placed with the supplier: goods may arrive.
    Ordered,
    /// Done: everything arrived, or what didn't was cancelled.
    Closed,
    /// Called off before anything arrived.
    Cancelled,
}

impl PurchaseOrderStatus {
    pub const ALL: [PurchaseOrderStatus; 4] = [
        PurchaseOrderStatus::Draft,
        PurchaseOrderStatus::Ordered,
        PurchaseOrderStatus::Closed,
        PurchaseOrderStatus::Cancelled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            PurchaseOrderStatus::Draft => "draft",
            PurchaseOrderStatus::Ordered => "ordered",
            PurchaseOrderStatus::Closed => "closed",
            PurchaseOrderStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == name)
    }
}

/// How much of an order has arrived, derived from its lines and stored for lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReceiptStatus {
    /// Nothing received or rejected yet.
    #[default]
    None,
    /// Some, not all.
    Partial,
    /// Every line accounted for by receipts (not by closing).
    Received,
}

impl ReceiptStatus {
    pub const ALL: [ReceiptStatus; 3] = [
        ReceiptStatus::None,
        ReceiptStatus::Partial,
        ReceiptStatus::Received,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ReceiptStatus::None => "none",
            ReceiptStatus::Partial => "partial",
            ReceiptStatus::Received => "received",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == name)
    }
}

/// What an extra cost is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostKind {
    Shipping,
    Duties,
    Fees,
    Other,
}

impl CostKind {
    pub const ALL: [CostKind; 4] = [
        CostKind::Shipping,
        CostKind::Duties,
        CostKind::Fees,
        CostKind::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            CostKind::Shipping => "shipping",
            CostKind::Duties => "duties",
            CostKind::Fees => "fees",
            CostKind::Other => "other",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

/// A line as given: checked by the order. The titles and SKU are snapshots of the
/// variant, which may be removed later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineDraft {
    pub variant: VariantId,
    pub product_title: String,
    pub variant_title: Option<String>,
    pub sku: Option<String>,
    /// The supplier's code for it.
    pub supplier_sku: Option<String>,
    pub quantity: u32,
    /// In the order's currency, net of tax.
    pub unit_cost: UnitCost,
}

/// One variant on an order, and how much of it is accounted for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrderLine {
    variant: VariantId,
    product_title: String,
    variant_title: Option<String>,
    sku: Option<String>,
    supplier_sku: Option<String>,
    quantity: u32,
    unit_cost: UnitCost,
    received: u32,
    rejected: u32,
    cancelled: u32,
    /// The extra costs' value the stock received on this line carries, in the store's
    /// currency in `UnitCost`'s scaled units: what a later change to the extra costs is
    /// measured against. `None` for a line received before it was kept, which is then
    /// measured against the order as it was before the change.
    extra_applied: Option<i64>,
}

impl PurchaseOrderLine {
    fn new(draft: LineDraft) -> Result<Self, PurchasingError> {
        if draft.quantity == 0 {
            return Err(PurchasingError::InvalidLineQuantity(draft.variant));
        }
        if draft.quantity > MAX_QUANTITY {
            return Err(PurchasingError::QuantityTooLarge {
                variant: draft.variant,
                max: MAX_QUANTITY,
            });
        }
        Ok(Self {
            variant: draft.variant,
            product_title: required_text("product title", &draft.product_title)?,
            variant_title: optional_text("variant title", draft.variant_title)?,
            sku: optional_text("SKU", draft.sku)?,
            supplier_sku: optional_text("supplier SKU", draft.supplier_sku)?,
            quantity: draft.quantity,
            unit_cost: draft.unit_cost,
            received: 0,
            rejected: 0,
            cancelled: 0,
            extra_applied: Some(0),
        })
    }

    pub fn variant(&self) -> VariantId {
        self.variant
    }

    pub fn product_title(&self) -> &str {
        &self.product_title
    }

    pub fn variant_title(&self) -> Option<&str> {
        self.variant_title.as_deref()
    }

    pub fn sku(&self) -> Option<&str> {
        self.sku.as_deref()
    }

    pub fn supplier_sku(&self) -> Option<&str> {
        self.supplier_sku.as_deref()
    }

    pub fn quantity(&self) -> u32 {
        self.quantity
    }

    pub fn unit_cost(&self) -> UnitCost {
        self.unit_cost
    }

    pub fn received(&self) -> u32 {
        self.received
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn cancelled(&self) -> u32 {
        self.cancelled
    }

    /// The extra costs' value the stock received on this line carries (see the field).
    pub fn extra_applied(&self) -> Option<i64> {
        self.extra_applied
    }

    /// Received or rejected: what receipts accounted for.
    fn arrived(&self) -> u64 {
        u64::from(self.received) + u64::from(self.rejected)
    }

    /// Still to come: ordered less received, rejected and cancelled, never below 0.
    /// Rejected units count as accounted for; a replacement is expected by raising the
    /// quantity.
    pub fn outstanding(&self) -> u32 {
        let done = self.arrived() + u64::from(self.cancelled);
        u32::try_from(u64::from(self.quantity).saturating_sub(done)).unwrap_or(0)
    }

    /// Ordered less cancelled: what extra costs are spread over.
    pub fn effective_quantity(&self) -> u32 {
        self.quantity.saturating_sub(self.cancelled)
    }

    /// What the line costs in the order's currency, rounded to the minor unit once.
    pub fn total(&self) -> Result<Money, PurchasingError> {
        Ok(self.unit_cost.total(self.effective_quantity())?)
    }

    pub fn draft(&self) -> LineDraft {
        LineDraft {
            variant: self.variant,
            product_title: self.product_title.clone(),
            variant_title: self.variant_title.clone(),
            sku: self.sku.clone(),
            supplier_sku: self.supplier_sku.clone(),
            quantity: self.quantity,
            unit_cost: self.unit_cost,
        }
    }
}

/// An extra cost as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostDraft {
    pub kind: CostKind,
    pub description: Option<String>,
    /// In the order's currency or the store's.
    pub amount: Money,
    pub allocation: CostAllocation,
}

/// Something paid to bring the goods in besides their price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraCost {
    kind: CostKind,
    description: Option<String>,
    amount: Money,
    allocation: CostAllocation,
}

impl ExtraCost {
    fn new(draft: CostDraft) -> Result<Self, PurchasingError> {
        if draft.amount.amount() < 0 {
            return Err(PurchasingError::NegativeCost);
        }
        Ok(Self {
            kind: draft.kind,
            description: optional_text("cost description", draft.description)?,
            amount: draft.amount,
            allocation: draft.allocation,
        })
    }

    pub fn kind(&self) -> CostKind {
        self.kind
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn amount(&self) -> Money {
        self.amount
    }

    pub fn allocation(&self) -> CostAllocation {
        self.allocation
    }

    pub fn draft(&self) -> CostDraft {
        CostDraft {
            kind: self.kind,
            description: self.description.clone(),
            amount: self.amount,
            allocation: self.allocation,
        }
    }
}

/// An order's editable fields as given: checked as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrderDraft {
    /// Where the goods go; required to order.
    pub destination: Option<LocationId>,
    pub currency: Currency,
    /// One unit of the order's currency in the store's; required to order in another
    /// currency than the store's.
    pub exchange_rate: Option<ExchangeRate>,
    pub payment_terms: Option<PaymentTermsDraft>,
    /// When the goods are due; ordering sets it from the supplier's lead time if unset.
    pub expected_at: Option<Timestamp>,
    /// The supplier's own number for the order.
    pub supplier_reference: Option<String>,
    /// Printed and emailed with the order.
    pub note_to_supplier: Option<String>,
    /// For staff only.
    pub note: Option<String>,
    pub lines: Vec<LineDraft>,
    pub costs: Vec<CostDraft>,
    /// For the total only: purchase tax is usually recovered, so it's in no cost.
    pub tax: Option<Money>,
}

impl PurchaseOrderDraft {
    /// An empty draft in `currency`.
    pub fn new(currency: Currency) -> Self {
        Self {
            destination: None,
            currency,
            exchange_rate: None,
            payment_terms: None,
            expected_at: None,
            supplier_reference: None,
            note_to_supplier: None,
            note: None,
            lines: Vec::new(),
            costs: Vec::new(),
            tax: None,
        }
    }
}

/// The checked editable fields.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fields {
    destination: Option<LocationId>,
    currency: Currency,
    exchange_rate: Option<ExchangeRate>,
    payment_terms: Option<PaymentTerms>,
    expected_at: Option<Timestamp>,
    supplier_reference: Option<String>,
    note_to_supplier: Option<String>,
    note: Option<String>,
    costs: Vec<ExtraCost>,
    tax: Option<Money>,
}

/// Checks `draft`; its lines come back new, with no counts.
fn check(draft: PurchaseOrderDraft) -> Result<(Fields, Vec<PurchaseOrderLine>), PurchasingError> {
    if draft.lines.len() > MAX_LINES {
        return Err(PurchasingError::TooManyLines { max: MAX_LINES });
    }
    if draft.costs.len() > MAX_COSTS {
        return Err(PurchasingError::TooManyCosts { max: MAX_COSTS });
    }
    let mut seen = HashSet::new();
    let mut lines = Vec::with_capacity(draft.lines.len());
    for line in draft.lines {
        if !seen.insert(line.variant) {
            return Err(PurchasingError::DuplicateLine(line.variant));
        }
        if line.unit_cost.currency() != draft.currency {
            return Err(PurchasingError::LineCurrencyMismatch {
                variant: line.variant,
                expected: draft.currency,
                got: line.unit_cost.currency(),
            });
        }
        lines.push(PurchaseOrderLine::new(line)?);
    }
    if let Some(tax) = draft.tax {
        if tax.amount() < 0 {
            return Err(PurchasingError::NegativeCost);
        }
        if tax.currency() != draft.currency {
            return Err(PurchasingError::TaxCurrencyMismatch {
                expected: draft.currency,
                got: tax.currency(),
            });
        }
    }
    let fields = Fields {
        destination: draft.destination,
        currency: draft.currency,
        exchange_rate: draft.exchange_rate,
        payment_terms: draft
            .payment_terms
            .map(|t| PaymentTerms::new(t.due, t.note))
            .transpose()?,
        expected_at: draft.expected_at,
        supplier_reference: optional_text("supplier reference", draft.supplier_reference)?,
        note_to_supplier: long_text(draft.note_to_supplier)?,
        note: long_text(draft.note)?,
        costs: draft
            .costs
            .into_iter()
            .map(ExtraCost::new)
            .collect::<Result<_, _>>()?,
        tax: draft.tax,
    };
    Ok((fields, lines))
}

/// Trimmed; blank is none.
pub(crate) fn long_text(text: Option<String>) -> Result<Option<String>, PurchasingError> {
    let Some(text) = text else { return Ok(None) };
    let text = text.trim();
    if text.chars().count() > MAX_NOTES_CHARS {
        return Err(PurchasingError::NotesTooLong {
            max: MAX_NOTES_CHARS,
        });
    }
    Ok((!text.is_empty()).then(|| text.to_string()))
}

/// Units of one variant a receipt accounts for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceivedQuantities {
    pub variant: VariantId,
    /// Into stock.
    pub accepted: u32,
    /// Refused (damaged, wrong): no stock.
    pub rejected: u32,
}

/// A recorded receipt as revaluing sees it: the rate it used and what it took in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptBasis {
    pub exchange_rate: Option<ExchangeRate>,
    pub lines: Vec<ReceivedQuantities>,
}

/// The value one variant's stock gains (or loses, negative) when what the extra costs
/// spread over its received units changes: in the store's currency, in `UnitCost`'s
/// scaled units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revaluation {
    pub variant: VariantId,
    pub value: i128,
}

/// What an order adds up to, in its currency, plus the extra costs paid in the store's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurchaseOrderTotals {
    /// The lines, ordered less cancelled.
    pub subtotal: Money,
    /// Extra costs in the order's currency.
    pub costs: Money,
    pub tax: Money,
    /// Subtotal, costs and tax.
    pub total: Money,
    /// Extra costs paid in the store's currency, when there are any.
    pub costs_in_store_currency: Option<Money>,
}

/// A store's order to one supplier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrder {
    id: PurchaseOrderId,
    version: Version,
    stamps: Option<Stamps>,
    number: PurchaseOrderNumber,
    supplier: SupplierId,
    /// The supplier's name as it was when ordered, for documents.
    supplier_name: Option<String>,
    fields: Fields,
    lines: Vec<PurchaseOrderLine>,
    status: PurchaseOrderStatus,
    receipt_status: ReceiptStatus,
    ordered_at: Option<Timestamp>,
    sent_at: Option<Timestamp>,
    /// The revision last sent: one below `revision` means the supplier has an older copy.
    sent_revision: Option<u32>,
    closed_at: Option<Timestamp>,
    cancelled_at: Option<Timestamp>,
    /// Edits since it was ordered.
    revision: u32,
    changes: Changes,
}

/// What happened since it was loaded, as events. Compares as always equal.
#[derive(Debug, Clone, Default)]
struct Changes {
    created: bool,
    fields: Vec<&'static str>,
    ordered: bool,
    receipt_status: Option<(ReceiptStatus, ReceiptStatus)>,
    closed: bool,
    cancelled: bool,
    sent: Option<Sending>,
}

impl Changes {
    fn field(&mut self, field: &'static str) {
        if !self.fields.contains(&field) {
            self.fields.push(field);
        }
    }

    fn any(&self) -> bool {
        self.created
            || !self.fields.is_empty()
            || self.ordered
            || self.receipt_status.is_some()
            || self.closed
            || self.cancelled
            || self.sent.is_some()
    }
}

impl PartialEq for Changes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changes {}

impl Recorded for PurchaseOrder {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        let changes = std::mem::take(&mut self.changes);
        let subject = Subject::new(PURCHASE_ORDER, self.id.as_uuid());
        let (purchase_order, supplier) = (self.id, self.supplier);
        let mut events = Vec::new();
        if changes.created {
            let event = PurchaseOrderCreated {
                purchase_order,
                supplier,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        if !changes.fields.is_empty() && !changes.created {
            let event = PurchaseOrderUpdated {
                purchase_order,
                supplier,
                fields: changes.fields,
                revision: self.revision,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        if changes.ordered {
            let event = PurchaseOrderOrdered {
                purchase_order,
                supplier,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        if let Some((from, to)) = changes.receipt_status
            && from != to
        {
            let event = PurchaseOrderReceiptStatusChanged {
                purchase_order,
                supplier,
                from: from.as_str(),
                to: to.as_str(),
            };
            events.push(PendingEvent::new(subject, &event));
        }
        if let Some(sending) = changes.sent {
            let event = PurchaseOrderSent {
                purchase_order,
                supplier,
                revision: self.revision,
                to: sending.to,
                cc: sending.cc,
                message: sending.message,
                ship_to: sending.ship_to,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        if changes.closed {
            let event = PurchaseOrderClosed {
                purchase_order,
                supplier,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        if changes.cancelled {
            let event = PurchaseOrderCancelled {
                purchase_order,
                supplier,
            };
            events.push(PendingEvent::new(subject, &event));
        }
        events
    }
}

impl Versioned for PurchaseOrder {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for PurchaseOrder {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

impl PurchaseOrder {
    /// A draft to `supplier`. Whether the supplier takes new orders (not archived) is the
    /// command's to check.
    pub fn new(
        id: PurchaseOrderId,
        number: PurchaseOrderNumber,
        supplier: SupplierId,
        draft: PurchaseOrderDraft,
    ) -> Result<Self, PurchasingError> {
        let (fields, lines) = check(draft)?;
        Ok(Self {
            id,
            version: Version::NEW,
            stamps: None,
            number,
            supplier,
            supplier_name: None,
            fields,
            lines,
            status: PurchaseOrderStatus::Draft,
            receipt_status: ReceiptStatus::None,
            ordered_at: None,
            sent_at: None,
            sent_revision: None,
            closed_at: None,
            cancelled_at: None,
            revision: 0,
            changes: Changes {
                created: true,
                ..Changes::default()
            },
        })
    }

    /// Replaces the editable fields with `draft`, checked as a whole.
    ///
    /// A draft changes freely. An ordered one keeps its currency
    /// (`purchase_order_currency_fixed`) and every line goods arrived on
    /// (`purchase_order_line_received`), with a quantity no lower than what arrived
    /// (`below_received`); it must still be orderable (`store` is the store's currency),
    /// and each edit counts a revision. One whose lines are all accounted for after the
    /// edit closes. A closed order takes changes to its extra costs only, and a cancelled
    /// one none (`purchase_order_not_open`).
    pub fn update(
        &mut self,
        draft: PurchaseOrderDraft,
        store: Option<Currency>,
        now: Timestamp,
    ) -> Result<(), PurchasingError> {
        let (ordered, closed) = match self.status {
            PurchaseOrderStatus::Draft => (false, false),
            PurchaseOrderStatus::Ordered => (true, false),
            PurchaseOrderStatus::Closed => (true, true),
            status @ PurchaseOrderStatus::Cancelled => {
                return Err(PurchasingError::NotOpen {
                    status: status.as_str(),
                });
            }
        };
        // Before the draft's own checks: its lines are in the new currency, which would
        // otherwise be refused as theirs rather than as the order's.
        if ordered && draft.currency != self.fields.currency {
            return Err(PurchasingError::CurrencyFixed);
        }
        let (fields, mut lines) = check(draft)?;
        // Lines keep their counts by variant.
        for old in &self.lines {
            match lines.iter_mut().find(|l| l.variant == old.variant) {
                Some(new) => {
                    // A line received over what it ordered keeps that quantity: only
                    // lowering it below what arrived is refused.
                    let minimum = old.received.saturating_add(old.rejected);
                    if new.quantity < minimum && new.quantity != old.quantity {
                        return Err(PurchasingError::BelowReceived {
                            variant: old.variant,
                            minimum,
                        });
                    }
                    new.received = old.received;
                    new.rejected = old.rejected;
                    new.cancelled = old.cancelled.min(new.quantity.saturating_sub(minimum));
                    new.extra_applied = old.extra_applied;
                }
                None if old.arrived() > 0 => {
                    return Err(PurchasingError::LineReceived(old.variant));
                }
                None => {}
            }
        }
        let old = &self.fields;
        let changed = [
            ("destination", fields.destination != old.destination),
            ("currency", fields.currency != old.currency),
            ("exchange_rate", fields.exchange_rate != old.exchange_rate),
            ("payment_terms", fields.payment_terms != old.payment_terms),
            ("expected_at", fields.expected_at != old.expected_at),
            (
                "supplier_reference",
                fields.supplier_reference != old.supplier_reference,
            ),
            (
                "note_to_supplier",
                fields.note_to_supplier != old.note_to_supplier,
            ),
            ("note", fields.note != old.note),
            ("lines", lines != self.lines),
            ("costs", fields.costs != old.costs),
            ("tax", fields.tax != old.tax),
        ];
        let changed: Vec<&'static str> = changed
            .into_iter()
            .filter(|(_, changed)| *changed)
            .map(|(field, _)| field)
            .collect();
        if changed.is_empty() {
            return Ok(());
        }
        // A closed order is done but for what its goods cost: its extra costs may still
        // change (a carrier's bill that came late).
        if closed && changed != ["costs"] {
            return Err(PurchasingError::NotOpen {
                status: self.status.as_str(),
            });
        }
        let mut next = self.clone();
        next.fields = fields;
        next.lines = lines;
        if ordered {
            next.check_orderable(store)?;
            next.revision += 1;
        }
        for field in changed {
            next.changes.field(field);
        }
        next.refresh_receipt_status();
        if ordered && !closed && next.all_accounted_for() {
            next.close_now(now);
        }
        *self = next;
        Ok(())
    }

    /// Whether anything changed since it was loaded (or it's new).
    pub fn is_changed(&self) -> bool {
        self.changes.any()
    }

    /// Places it with the supplier: it needs a destination, a line, the store's currency
    /// and, in another currency, an exchange rate. The supplier's name is kept for
    /// documents; an unset expected date becomes now plus `lead_time_days`.
    pub fn order(
        &mut self,
        store: Option<Currency>,
        supplier_name: &str,
        lead_time_days: Option<u32>,
        now: Timestamp,
    ) -> Result<(), PurchasingError> {
        if self.status != PurchaseOrderStatus::Draft {
            return Err(PurchasingError::NotDraft {
                status: self.status.as_str(),
            });
        }
        self.check_orderable(store)?;
        self.supplier_name = Some(required_text("supplier name", supplier_name)?);
        if self.fields.expected_at.is_none()
            && let Some(days) = lead_time_days
        {
            self.fields.expected_at = Some(now.plus_secs(i64::from(days) * 86_400));
        }
        self.status = PurchaseOrderStatus::Ordered;
        self.ordered_at = Some(now);
        self.changes.ordered = true;
        Ok(())
    }

    /// Emails it to its supplier: a draft is ordered first (`order`'s rules, with the
    /// same arguments), then `purchase_order.sent` records `sending` for the mailer and
    /// `sent_at` is now. A closed or cancelled order isn't sent
    /// (`purchase_order_not_open`). All or nothing.
    pub fn send(
        &mut self,
        sending: Sending,
        store: Option<Currency>,
        supplier_name: &str,
        lead_time_days: Option<u32>,
        now: Timestamp,
    ) -> Result<(), PurchasingError> {
        let sending = sending.checked()?;
        match self.status {
            PurchaseOrderStatus::Draft => self.order(store, supplier_name, lead_time_days, now)?,
            PurchaseOrderStatus::Ordered => {}
            status @ (PurchaseOrderStatus::Closed | PurchaseOrderStatus::Cancelled) => {
                return Err(PurchasingError::NotOpen {
                    status: status.as_str(),
                });
            }
        }
        self.sent_at = Some(now);
        self.sent_revision = Some(self.revision);
        self.changes.sent = Some(sending);
        Ok(())
    }

    /// What an ordered order must hold. A rate given for an order in the store's own
    /// currency is dropped: there's nothing to convert.
    fn check_orderable(&mut self, store: Option<Currency>) -> Result<(), PurchasingError> {
        let store = store.ok_or(PurchasingError::StoreCurrencyRequired)?;
        if self.fields.destination.is_none() {
            return Err(PurchasingError::DestinationRequired);
        }
        if self.lines.is_empty() {
            return Err(PurchasingError::NoLines);
        }
        if self.fields.currency == store {
            self.fields.exchange_rate = None;
        } else if self.fields.exchange_rate.is_none() {
            return Err(PurchasingError::ExchangeRateRequired {
                currency: self.fields.currency,
                store,
            });
        }
        if let Some(cost) =
            self.fields.costs.iter().find(|c| {
                c.amount.currency() != self.fields.currency && c.amount.currency() != store
            })
        {
            return Err(PurchasingError::ExtraCostCurrency {
                order: self.fields.currency,
                store,
                got: cost.amount.currency(),
            });
        }
        Ok(())
    }

    /// Ends an ordered order early: whatever is outstanding on each line is cancelled.
    pub fn close(&mut self, now: Timestamp) -> Result<(), PurchasingError> {
        if self.status != PurchaseOrderStatus::Ordered {
            return Err(PurchasingError::NotOrdered {
                status: self.status.as_str(),
            });
        }
        for line in &mut self.lines {
            line.cancelled = line.cancelled.saturating_add(line.outstanding());
        }
        self.refresh_receipt_status();
        self.close_now(now);
        Ok(())
    }

    fn close_now(&mut self, now: Timestamp) {
        self.status = PurchaseOrderStatus::Closed;
        self.closed_at = Some(now);
        self.changes.closed = true;
    }

    /// Calls a draft or ordered order off. Refused once anything arrived
    /// (`purchase_order_received`): close it instead.
    pub fn cancel(&mut self, now: Timestamp) -> Result<(), PurchasingError> {
        match self.status {
            PurchaseOrderStatus::Draft | PurchaseOrderStatus::Ordered => {}
            status => {
                return Err(PurchasingError::NotOpen {
                    status: status.as_str(),
                });
            }
        }
        if self.lines.iter().any(|l| l.arrived() > 0) {
            return Err(PurchasingError::AlreadyReceived);
        }
        self.status = PurchaseOrderStatus::Cancelled;
        self.cancelled_at = Some(now);
        self.changes.cancelled = true;
        Ok(())
    }

    /// Checks it may be deleted: only drafts are.
    pub fn check_deletable(&self) -> Result<(), PurchasingError> {
        if self.status != PurchaseOrderStatus::Draft {
            return Err(PurchasingError::NotDraft {
                status: self.status.as_str(),
            });
        }
        Ok(())
    }

    /// Counts what a receipt brought on an ordered order. Each variant must be on the
    /// order once (`unknown_purchase_order_line`, `duplicate_line`) with at least one unit
    /// (`empty_receipt_line`). More than a line's outstanding is refused with every such
    /// line's outstanding (`over_received`) unless `allow_over`. The order closes once
    /// every line is accounted for. All or nothing.
    pub fn record_receipt(
        &mut self,
        lines: &[ReceivedQuantities],
        allow_over: bool,
        now: Timestamp,
    ) -> Result<(), PurchasingError> {
        if self.status != PurchaseOrderStatus::Ordered {
            return Err(PurchasingError::NotOrdered {
                status: self.status.as_str(),
            });
        }
        let mut next = self.lines.clone();
        let mut seen = HashSet::new();
        let mut over = Vec::new();
        for received in lines {
            if !seen.insert(received.variant) {
                return Err(PurchasingError::DuplicateLine(received.variant));
            }
            let Some(line) = next.iter_mut().find(|l| l.variant == received.variant) else {
                return Err(PurchasingError::UnknownLine(received.variant));
            };
            let units = u64::from(received.accepted) + u64::from(received.rejected);
            if units == 0 {
                return Err(PurchasingError::EmptyReceiptLine(received.variant));
            }
            if units > u64::from(line.outstanding()) {
                over.push(Outstanding {
                    variant: line.variant,
                    outstanding: line.outstanding(),
                });
            }
            line.received = add_count(line.variant, line.received, received.accepted)?;
            line.rejected = add_count(line.variant, line.rejected, received.rejected)?;
        }
        if !over.is_empty() && !allow_over {
            return Err(PurchasingError::OverReceived(over));
        }
        self.lines = next;
        self.refresh_receipt_status();
        if self.all_accounted_for() {
            self.close_now(now);
        }
        Ok(())
    }

    /// Takes a receipt's counts back off the lines: a closed order opens again.
    /// Refused on a cancelled order, or if a line counts fewer than would be taken back
    /// (`receipt_mismatch`). All or nothing.
    pub fn take_back_receipt(
        &mut self,
        lines: &[ReceivedQuantities],
    ) -> Result<(), PurchasingError> {
        match self.status {
            PurchaseOrderStatus::Ordered | PurchaseOrderStatus::Closed => {}
            status => {
                return Err(PurchasingError::NotOpen {
                    status: status.as_str(),
                });
            }
        }
        let mut next = self.lines.clone();
        for taken in lines {
            let Some(line) = next.iter_mut().find(|l| l.variant == taken.variant) else {
                return Err(PurchasingError::UnknownLine(taken.variant));
            };
            let mismatch = || PurchasingError::TakeBackMismatch(taken.variant);
            line.received = line
                .received
                .checked_sub(taken.accepted)
                .ok_or_else(mismatch)?;
            line.rejected = line
                .rejected
                .checked_sub(taken.rejected)
                .ok_or_else(mismatch)?;
        }
        self.lines = next;
        if self.status == PurchaseOrderStatus::Closed {
            self.status = PurchaseOrderStatus::Ordered;
            self.closed_at = None;
            self.changes.field("status");
        }
        self.refresh_receipt_status();
        Ok(())
    }

    /// Every line's outstanding is 0.
    fn all_accounted_for(&self) -> bool {
        self.lines.iter().all(|l| l.outstanding() == 0)
    }

    fn refresh_receipt_status(&mut self) {
        let status = if self.lines.iter().all(|l| l.arrived() == 0) {
            ReceiptStatus::None
        } else if self
            .lines
            .iter()
            .all(|l| l.arrived() >= u64::from(l.quantity))
        {
            ReceiptStatus::Received
        } else {
            ReceiptStatus::Partial
        };
        if status != self.receipt_status {
            let from = self
                .changes
                .receipt_status
                .map_or(self.receipt_status, |(from, _)| from);
            self.changes.receipt_status = Some((from, status));
            self.receipt_status = status;
        }
    }

    /// What it adds up to. `store` is the store's currency, for the extra costs paid in it.
    pub fn totals(&self, store: Option<Currency>) -> Result<PurchaseOrderTotals, PurchasingError> {
        let currency = self.fields.currency;
        let mut subtotal = Money::zero(currency);
        for line in &self.lines {
            subtotal = subtotal.checked_add(line.total()?)?;
        }
        let mut costs = Money::zero(currency);
        let mut in_store: Option<Money> = None;
        for cost in &self.fields.costs {
            if cost.amount.currency() == currency {
                costs = costs.checked_add(cost.amount)?;
            } else if Some(cost.amount.currency()) == store {
                let sum = in_store.unwrap_or_else(|| Money::zero(cost.amount.currency()));
                in_store = Some(sum.checked_add(cost.amount)?);
            }
        }
        let tax = self.fields.tax.unwrap_or_else(|| Money::zero(currency));
        let total = subtotal.checked_add(costs)?.checked_add(tax)?;
        Ok(PurchaseOrderTotals {
            subtotal,
            costs,
            tax,
            total,
            costs_in_store_currency: in_store,
        })
    }

    /// Each line's landed unit cost in `store` (the store's currency), in line order:
    /// its unit cost and share of the extra costs, at `rate` (the order's own, or a
    /// receipt's). `None` for every line when the order needs a rate and has none.
    pub fn landed_costs(
        &self,
        store: Currency,
        rate: Option<ExchangeRate>,
    ) -> Result<Vec<Option<UnitCost>>, PurchasingError> {
        let basis: Vec<LineBasis> = self
            .lines
            .iter()
            .map(|l| LineBasis {
                quantity: l.effective_quantity(),
                unit_cost: l.unit_cost,
            })
            .collect();
        let shares: Vec<CostShare> = self
            .fields
            .costs
            .iter()
            .map(|c| CostShare {
                amount: c.amount,
                allocation: c.allocation,
            })
            .collect();
        let in_order = extra_per_unit(&basis, &shares, self.fields.currency)?;
        let in_store = if store == self.fields.currency {
            vec![UnitCost::zero(store); basis.len()]
        } else {
            extra_per_unit(&basis, &shares, store)?
        };
        self.lines
            .iter()
            .zip(in_order)
            .zip(in_store)
            .map(|((line, order_extra), store_extra)| {
                Ok(landed_unit_cost(
                    line.unit_cost,
                    order_extra,
                    store_extra,
                    rate,
                    store,
                )?)
            })
            .collect()
    }

    /// Per variant, what each unit carries of the extra costs, in `store`, at `rate`:
    /// its landed cost less the landed cost of its unit cost alone, in `UnitCost`'s
    /// scaled units. `None` for every line when the order needs a rate and has none.
    pub fn extras_per_unit(
        &self,
        store: Currency,
        rate: Option<ExchangeRate>,
    ) -> Result<HashMap<VariantId, Option<i128>>, PurchasingError> {
        let landed = self.landed_costs(store, rate)?;
        self.lines
            .iter()
            .zip(landed)
            .map(|(line, landed)| {
                let bare = self.bare_landed_cost(line.variant, store, rate)?;
                let extra = match (landed, bare) {
                    (Some(landed), Some(bare)) => {
                        Some(i128::from(landed.scaled()) - i128::from(bare.scaled()))
                    }
                    _ => None,
                };
                Ok((line.variant, extra))
            })
            .collect()
    }

    /// A line's unit cost alone in `store`, at `rate`, without any extra cost: `None`
    /// for a variant not on the order, or when a rate is needed and missing.
    pub fn bare_landed_cost(
        &self,
        variant: VariantId,
        store: Currency,
        rate: Option<ExchangeRate>,
    ) -> Result<Option<UnitCost>, PurchasingError> {
        let Some(line) = self.line(variant) else {
            return Ok(None);
        };
        Ok(landed_unit_cost(
            line.unit_cost,
            UnitCost::zero(self.fields.currency),
            UnitCost::zero(store),
            rate,
            store,
        )?)
    }

    /// The extra costs' value the stock received on each line should carry now: the
    /// accepted units of `receipts` (the recorded ones, oldest first) at the extra per
    /// unit the order spreads now, at each receipt's rate. Only as many units as the line
    /// spreads over carry it (`extra_bearing`): units received over the order came in at
    /// their unit cost alone, or the stock would carry more than was paid.
    pub fn extra_due(
        &self,
        store: Currency,
        receipts: &[ReceiptBasis],
    ) -> Result<HashMap<VariantId, i128>, PurchasingError> {
        let mut due: HashMap<VariantId, i128> = HashMap::new();
        let mut counted_before: HashMap<VariantId, u32> = HashMap::new();
        for receipt in receipts {
            let extras = self.extras_per_unit(store, receipt.exchange_rate)?;
            for counted in receipt.lines.iter().filter(|l| l.accepted > 0) {
                let before = counted_before.entry(counted.variant).or_default();
                let bearing = self.bearing_after(counted.variant, *before, counted.accepted);
                *before = before.saturating_add(counted.accepted);
                let Some(Some(extra)) = extras.get(&counted.variant) else {
                    continue;
                };
                *due.entry(counted.variant).or_default() += i128::from(bearing) * extra;
            }
        }
        Ok(due)
    }

    /// How many of `accepted` units of `variant` arriving now carry the extra costs: no
    /// more than its line spreads them over (ordered less cancelled) less what arrived
    /// before. Call it before `record_receipt`.
    pub fn extra_bearing(&self, variant: VariantId, accepted: u32) -> u32 {
        let before = self.line(variant).map_or(0, |l| l.received);
        self.bearing_after(variant, before, accepted)
    }

    fn bearing_after(&self, variant: VariantId, before: u32, accepted: u32) -> u32 {
        let spread = self.line(variant).map_or(0, |l| l.effective_quantity());
        accepted.min(spread.saturating_sub(before))
    }

    /// Brings the extra value each line's received stock carries to what's due now
    /// (`extra_due` over `receipts`, the recorded ones) and returns the differences for
    /// the caller to put on the stock: an extra cost added, changed or removed after
    /// goods arrived, or a spread that changed (closing cancels what's outstanding, so
    /// fewer units share the costs). A line without a kept value is measured against
    /// `before` (what was due before the change), or taken as already right.
    pub fn revalue(
        &mut self,
        store: Currency,
        receipts: &[ReceiptBasis],
        before: &HashMap<VariantId, i128>,
    ) -> Result<Vec<Revaluation>, PurchasingError> {
        let due = self.extra_due(store, receipts)?;
        let mut next = self.lines.clone();
        let mut changes = Vec::new();
        for line in &mut next {
            let now = due.get(&line.variant).copied().unwrap_or(0);
            let applied = match line.extra_applied {
                Some(applied) => i128::from(applied),
                None => before.get(&line.variant).copied().unwrap_or(now),
            };
            line.extra_applied = Some(scaled_value(now)?);
            if now != applied {
                changes.push(Revaluation {
                    variant: line.variant,
                    value: now - applied,
                });
            }
        }
        self.lines = next;
        Ok(changes)
    }

    /// Counts the extra value `accepted` units of `variant` brought in at `per_unit`
    /// (what each carried of the extra costs; units over what the line spreads over
    /// carried none): call it after `record_receipt`. A line without a kept value starts
    /// one only when these are its first units.
    pub fn add_extra_applied(
        &mut self,
        variant: VariantId,
        accepted: u32,
        per_unit: i128,
    ) -> Result<(), PurchasingError> {
        let before = self
            .line(variant)
            .map_or(0, |l| l.received.saturating_sub(accepted));
        let bearing = self.bearing_after(variant, before, accepted);
        let Some(line) = self.lines.iter_mut().find(|l| l.variant == variant) else {
            return Ok(());
        };
        let value = i128::from(bearing) * per_unit;
        line.extra_applied = match line.extra_applied {
            Some(applied) => Some(scaled_value(i128::from(applied) + value)?),
            None if line.received == accepted => Some(scaled_value(value)?),
            None => None,
        };
        Ok(())
    }

    /// Takes off the extra value `accepted` units of `variant` carried at `per_unit`,
    /// for a receipt taken back.
    pub fn take_extra_applied(
        &mut self,
        variant: VariantId,
        accepted: u32,
        per_unit: i128,
    ) -> Result<(), PurchasingError> {
        let Some(line) = self.lines.iter_mut().find(|l| l.variant == variant) else {
            return Ok(());
        };
        if let Some(applied) = line.extra_applied {
            let value = i128::from(applied) - i128::from(accepted) * per_unit;
            line.extra_applied = Some(scaled_value(value)?);
        }
        Ok(())
    }

    /// The order's editable fields as a draft, to change some and `update`.
    pub fn draft(&self) -> PurchaseOrderDraft {
        let f = &self.fields;
        PurchaseOrderDraft {
            destination: f.destination,
            currency: f.currency,
            exchange_rate: f.exchange_rate,
            payment_terms: f.payment_terms.as_ref().map(|t| PaymentTermsDraft {
                due: t.due(),
                note: t.note().map(str::to_string),
            }),
            expected_at: f.expected_at,
            supplier_reference: f.supplier_reference.clone(),
            note_to_supplier: f.note_to_supplier.clone(),
            note: f.note.clone(),
            lines: self.lines.iter().map(PurchaseOrderLine::draft).collect(),
            costs: f.costs.iter().map(ExtraCost::draft).collect(),
            tax: f.tax,
        }
    }

    pub fn id(&self) -> PurchaseOrderId {
        self.id
    }

    pub fn number(&self) -> PurchaseOrderNumber {
        self.number
    }

    pub fn supplier(&self) -> SupplierId {
        self.supplier
    }

    /// The supplier's name as it was when ordered; `None` on a draft.
    pub fn supplier_name(&self) -> Option<&str> {
        self.supplier_name.as_deref()
    }

    pub fn destination(&self) -> Option<LocationId> {
        self.fields.destination
    }

    pub fn currency(&self) -> Currency {
        self.fields.currency
    }

    pub fn exchange_rate(&self) -> Option<ExchangeRate> {
        self.fields.exchange_rate
    }

    pub fn payment_terms(&self) -> Option<&PaymentTerms> {
        self.fields.payment_terms.as_ref()
    }

    pub fn expected_at(&self) -> Option<Timestamp> {
        self.fields.expected_at
    }

    pub fn supplier_reference(&self) -> Option<&str> {
        self.fields.supplier_reference.as_deref()
    }

    pub fn note_to_supplier(&self) -> Option<&str> {
        self.fields.note_to_supplier.as_deref()
    }

    pub fn note(&self) -> Option<&str> {
        self.fields.note.as_deref()
    }

    pub fn lines(&self) -> &[PurchaseOrderLine] {
        &self.lines
    }

    pub fn line(&self, variant: VariantId) -> Option<&PurchaseOrderLine> {
        self.lines.iter().find(|l| l.variant == variant)
    }

    pub fn costs(&self) -> &[ExtraCost] {
        &self.fields.costs
    }

    pub fn tax(&self) -> Option<Money> {
        self.fields.tax
    }

    pub fn status(&self) -> PurchaseOrderStatus {
        self.status
    }

    pub fn receipt_status(&self) -> ReceiptStatus {
        self.receipt_status
    }

    pub fn ordered_at(&self) -> Option<Timestamp> {
        self.ordered_at
    }

    pub fn sent_at(&self) -> Option<Timestamp> {
        self.sent_at
    }

    /// Whether it's priced against the store's base currency: ordered or closed (its
    /// landed costs are in it), or a draft with a rate to it. While any is, that currency
    /// stays as it is.
    pub fn names_store_currency(&self) -> bool {
        match self.status {
            PurchaseOrderStatus::Ordered | PurchaseOrderStatus::Closed => true,
            PurchaseOrderStatus::Draft => self.fields.exchange_rate.is_some(),
            PurchaseOrderStatus::Cancelled => false,
        }
    }

    /// The revision the supplier was last sent; `None` before it was sent.
    pub fn sent_revision(&self) -> Option<u32> {
        self.sent_revision
    }

    pub fn closed_at(&self) -> Option<Timestamp> {
        self.closed_at
    }

    pub fn cancelled_at(&self) -> Option<Timestamp> {
        self.cancelled_at
    }

    pub fn revision(&self) -> u32 {
        self.revision
    }
}

/// A value in `UnitCost`'s scaled units as stored.
fn scaled_value(value: i128) -> Result<i64, PurchasingError> {
    i64::try_from(value).map_err(|_| PurchasingError::Money(hypha_core::MoneyError::Overflow))
}

/// `count + more`, refused past `MAX_QUANTITY`.
fn add_count(variant: VariantId, count: u32, more: u32) -> Result<u32, PurchasingError> {
    count
        .checked_add(more)
        .filter(|sum| *sum <= MAX_QUANTITY)
        .ok_or(PurchasingError::QuantityTooLarge {
            variant,
            max: MAX_QUANTITY,
        })
}

/// A line as storage keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineSnapshot {
    pub variant: VariantId,
    pub product_title: String,
    pub variant_title: Option<String>,
    pub sku: Option<String>,
    pub supplier_sku: Option<String>,
    pub quantity: u32,
    pub unit_cost: UnitCost,
    pub received: u32,
    pub rejected: u32,
    pub cancelled: u32,
    /// The extra costs' value its received stock carries (`PurchaseOrderLine`'s field).
    pub extra_applied: Option<i64>,
}

/// Plain-data form of a `PurchaseOrder` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrderSnapshot {
    pub id: PurchaseOrderId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub number: PurchaseOrderNumber,
    pub supplier: SupplierId,
    pub supplier_name: Option<String>,
    pub destination: Option<LocationId>,
    pub currency: Currency,
    pub exchange_rate: Option<ExchangeRate>,
    pub payment_terms: Option<PaymentTermsDraft>,
    pub expected_at: Option<Timestamp>,
    pub supplier_reference: Option<String>,
    pub note_to_supplier: Option<String>,
    pub note: Option<String>,
    pub lines: Vec<LineSnapshot>,
    pub costs: Vec<CostDraft>,
    pub tax: Option<Money>,
    pub status: PurchaseOrderStatus,
    pub receipt_status: ReceiptStatus,
    pub ordered_at: Option<Timestamp>,
    pub sent_at: Option<Timestamp>,
    pub sent_revision: Option<u32>,
    pub closed_at: Option<Timestamp>,
    pub cancelled_at: Option<Timestamp>,
    pub revision: u32,
}

impl PurchaseOrder {
    pub fn snapshot(&self) -> PurchaseOrderSnapshot {
        let draft = self.draft();
        PurchaseOrderSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            number: self.number,
            supplier: self.supplier,
            supplier_name: self.supplier_name.clone(),
            destination: draft.destination,
            currency: draft.currency,
            exchange_rate: draft.exchange_rate,
            payment_terms: draft.payment_terms,
            expected_at: draft.expected_at,
            supplier_reference: draft.supplier_reference,
            note_to_supplier: draft.note_to_supplier,
            note: draft.note,
            lines: self
                .lines
                .iter()
                .map(|l| LineSnapshot {
                    variant: l.variant,
                    product_title: l.product_title.clone(),
                    variant_title: l.variant_title.clone(),
                    sku: l.sku.clone(),
                    supplier_sku: l.supplier_sku.clone(),
                    quantity: l.quantity,
                    unit_cost: l.unit_cost,
                    received: l.received,
                    rejected: l.rejected,
                    cancelled: l.cancelled,
                    extra_applied: l.extra_applied,
                })
                .collect(),
            costs: draft.costs,
            tax: draft.tax,
            status: self.status,
            receipt_status: self.receipt_status,
            ordered_at: self.ordered_at,
            sent_at: self.sent_at,
            sent_revision: self.sent_revision,
            closed_at: self.closed_at,
            cancelled_at: self.cancelled_at,
            revision: self.revision,
        }
    }

    /// Rebuilds an order from storage, checked like a draft (lines, costs, texts), its
    /// counts as stored.
    pub fn restore(s: PurchaseOrderSnapshot) -> Result<PurchaseOrder, PurchasingError> {
        let counts: Vec<(u32, u32, u32, Option<i64>)> = s
            .lines
            .iter()
            .map(|l| (l.received, l.rejected, l.cancelled, l.extra_applied))
            .collect();
        let draft = PurchaseOrderDraft {
            destination: s.destination,
            currency: s.currency,
            exchange_rate: s.exchange_rate,
            payment_terms: s.payment_terms,
            expected_at: s.expected_at,
            supplier_reference: s.supplier_reference,
            note_to_supplier: s.note_to_supplier,
            note: s.note,
            lines: s
                .lines
                .into_iter()
                .map(|l| LineDraft {
                    variant: l.variant,
                    product_title: l.product_title,
                    variant_title: l.variant_title,
                    sku: l.sku,
                    supplier_sku: l.supplier_sku,
                    quantity: l.quantity,
                    unit_cost: l.unit_cost,
                })
                .collect(),
            costs: s.costs,
            tax: s.tax,
        };
        let (fields, mut lines) = check(draft)?;
        for (line, (received, rejected, cancelled, extra)) in lines.iter_mut().zip(counts) {
            line.received = received;
            line.rejected = rejected;
            line.cancelled = cancelled;
            line.extra_applied = extra;
        }
        Ok(PurchaseOrder {
            id: s.id,
            version: s.version,
            stamps: s.stamps,
            number: s.number,
            supplier: s.supplier,
            supplier_name: s.supplier_name,
            fields,
            lines,
            status: s.status,
            receipt_status: s.receipt_status,
            ordered_at: s.ordered_at,
            sent_at: s.sent_at,
            sent_revision: s.sent_revision,
            closed_at: s.closed_at,
            cancelled_at: s.cancelled_at,
            revision: s.revision,
            changes: Changes::default(),
        })
    }

    /// What a list shows of it.
    pub fn summary(&self) -> PurchaseOrderSummary {
        PurchaseOrderSummary {
            id: self.id,
            number: self.number,
            supplier: self.supplier,
            supplier_name: self.supplier_name.clone(),
            destination: self.fields.destination,
            currency: self.fields.currency,
            status: self.status,
            receipt_status: self.receipt_status,
            expected_at: self.fields.expected_at,
            ordered_at: self.ordered_at,
            line_count: self.lines.len(),
            total: self.totals(None).map(|t| t.total).ok(),
            created_at: self.stamps.map(|s| s.created_at),
        }
    }
}

/// A purchase order as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrderSummary {
    pub id: PurchaseOrderId,
    pub number: PurchaseOrderNumber,
    pub supplier: SupplierId,
    pub supplier_name: Option<String>,
    pub destination: Option<LocationId>,
    pub currency: Currency,
    pub status: PurchaseOrderStatus,
    pub receipt_status: ReceiptStatus,
    pub expected_at: Option<Timestamp>,
    pub ordered_at: Option<Timestamp>,
    pub line_count: usize,
    /// Subtotal, extra costs in its currency and tax; `None` past the largest amount.
    pub total: Option<Money>,
    pub created_at: Option<Timestamp>,
}

#[cfg(test)]
mod tests {
    use hypha_core::{MoneyError, TextError};
    use uuid::Uuid;

    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn variant(n: u128) -> VariantId {
        VariantId::from_uuid(id(n))
    }

    fn eur(minor: &str) -> UnitCost {
        UnitCost::parse(minor, Currency::EUR).unwrap()
    }

    fn line(n: u128, quantity: u32, minor: &str) -> LineDraft {
        LineDraft {
            variant: variant(n),
            product_title: "Grow kit".into(),
            variant_title: Some("Oyster".into()),
            sku: Some(format!("GK-{n}")),
            supplier_sku: None,
            quantity,
            unit_cost: eur(minor),
        }
    }

    fn draft(lines: Vec<LineDraft>) -> PurchaseOrderDraft {
        PurchaseOrderDraft {
            destination: Some(LocationId::from_uuid(id(50))),
            lines,
            ..PurchaseOrderDraft::new(Currency::EUR)
        }
    }

    fn po(lines: Vec<LineDraft>) -> PurchaseOrder {
        PurchaseOrder::new(
            PurchaseOrderId::from_uuid(id(1)),
            PurchaseOrderNumber::new(1001).unwrap(),
            SupplierId::from_uuid(id(2)),
            draft(lines),
        )
        .unwrap()
    }

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_unix_micros(secs * 1_000_000)
    }

    fn ordered(lines: Vec<LineDraft>) -> PurchaseOrder {
        let mut po = po(lines);
        po.order(Some(Currency::EUR), "Spore Supply", None, at(100))
            .unwrap();
        po.take_events();
        po
    }

    fn got(n: u128, accepted: u32, rejected: u32) -> ReceivedQuantities {
        ReceivedQuantities {
            variant: variant(n),
            accepted,
            rejected,
        }
    }

    fn event_types(po: &mut PurchaseOrder) -> Vec<String> {
        po.take_events()
            .iter()
            .map(|e| e.event_type.to_string())
            .collect()
    }

    #[test]
    fn numbers_start_at_1_and_show_with_a_prefix() {
        assert_eq!(
            PurchaseOrderNumber::new(0),
            Err(PurchasingError::InvalidNumber(0))
        );
        assert_eq!(
            PurchaseOrderNumber::new(1001).unwrap().to_string(),
            "PO-1001"
        );
    }

    #[test]
    fn a_new_order_is_a_draft_and_records_its_creation() {
        let mut po = po(vec![line(10, 5, "100")]);
        assert_eq!(po.status(), PurchaseOrderStatus::Draft);
        assert_eq!(po.receipt_status(), ReceiptStatus::None);
        assert_eq!(event_types(&mut po), ["purchase_order.created"]);
    }

    #[test]
    fn one_line_per_variant() {
        let result = PurchaseOrder::new(
            PurchaseOrderId::from_uuid(id(1)),
            PurchaseOrderNumber::new(1).unwrap(),
            SupplierId::from_uuid(id(2)),
            draft(vec![line(10, 1, "1"), line(10, 2, "1")]),
        );
        assert_eq!(result, Err(PurchasingError::DuplicateLine(variant(10))));
    }

    #[test]
    fn lines_need_a_quantity_and_the_orders_currency() {
        let new = |lines| {
            PurchaseOrder::new(
                PurchaseOrderId::from_uuid(id(1)),
                PurchaseOrderNumber::new(1).unwrap(),
                SupplierId::from_uuid(id(2)),
                draft(lines),
            )
        };
        assert_eq!(
            new(vec![line(10, 0, "1")]),
            Err(PurchasingError::InvalidLineQuantity(variant(10)))
        );
        let mut usd = line(10, 1, "1");
        usd.unit_cost = UnitCost::parse("1", Currency::USD).unwrap();
        assert_eq!(
            new(vec![usd]),
            Err(PurchasingError::LineCurrencyMismatch {
                variant: variant(10),
                expected: Currency::EUR,
                got: Currency::USD,
            })
        );
        let mut untitled = line(10, 1, "1");
        untitled.product_title = " ".into();
        assert_eq!(
            new(vec![untitled]),
            Err(PurchasingError::Text(TextError::Missing("product title")))
        );
    }

    #[test]
    fn at_most_1000_lines_and_20_costs() {
        let lines: Vec<_> = (0..=1000).map(|n| line(n, 1, "1")).collect();
        let mut too_many = draft(lines);
        assert_eq!(
            check(too_many.clone()).map(|_| ()),
            Err(PurchasingError::TooManyLines { max: 1000 })
        );
        too_many.lines.truncate(1000);
        assert!(check(too_many.clone()).is_ok());
        too_many.costs = vec![
            CostDraft {
                kind: CostKind::Shipping,
                description: None,
                amount: Money::new(1, Currency::EUR).unwrap(),
                allocation: CostAllocation::ByValue,
            };
            21
        ];
        assert_eq!(
            check(too_many).map(|_| ()),
            Err(PurchasingError::TooManyCosts { max: 20 })
        );
    }

    #[test]
    fn costs_and_tax_are_never_negative() {
        let mut d = draft(vec![line(10, 1, "1")]);
        d.costs = vec![CostDraft {
            kind: CostKind::Fees,
            description: None,
            amount: Money::new(-1, Currency::EUR).unwrap(),
            allocation: CostAllocation::None,
        }];
        assert_eq!(
            check(d.clone()).map(|_| ()),
            Err(PurchasingError::NegativeCost)
        );
        d.costs.clear();
        d.tax = Some(Money::new(-1, Currency::EUR).unwrap());
        assert_eq!(
            check(d.clone()).map(|_| ()),
            Err(PurchasingError::NegativeCost)
        );
        d.tax = Some(Money::new(1, Currency::USD).unwrap());
        assert_eq!(
            check(d).map(|_| ()),
            Err(PurchasingError::TaxCurrencyMismatch {
                expected: Currency::EUR,
                got: Currency::USD
            })
        );
    }

    #[test]
    fn ordering_needs_a_destination_a_line_and_the_stores_currency() {
        let mut no_lines = po(vec![]);
        assert_eq!(
            no_lines.order(Some(Currency::EUR), "S", None, at(1)),
            Err(PurchasingError::NoLines)
        );
        let mut nowhere = po(vec![line(10, 1, "1")]);
        let mut d = nowhere.draft();
        d.destination = None;
        nowhere.update(d, None, at(1)).unwrap();
        assert_eq!(
            nowhere.order(Some(Currency::EUR), "S", None, at(1)),
            Err(PurchasingError::DestinationRequired)
        );
        let mut no_store = po(vec![line(10, 1, "1")]);
        assert_eq!(
            no_store.order(None, "S", None, at(1)),
            Err(PurchasingError::StoreCurrencyRequired)
        );
        assert_eq!(no_store.status(), PurchaseOrderStatus::Draft);
    }

    #[test]
    fn ordering_in_another_currency_needs_a_rate() {
        let mut po = po(vec![line(10, 1, "1")]);
        assert_eq!(
            po.order(Some(Currency::USD), "S", None, at(1)),
            Err(PurchasingError::ExchangeRateRequired {
                currency: Currency::EUR,
                store: Currency::USD
            })
        );
        let mut d = po.draft();
        d.exchange_rate = Some(ExchangeRate::parse("1.1").unwrap());
        po.update(d, Some(Currency::USD), at(1)).unwrap();
        assert_eq!(po.order(Some(Currency::USD), "S", None, at(1)), Ok(()));
    }

    #[test]
    fn a_rate_in_the_stores_own_currency_is_dropped() {
        let mut po = po(vec![line(10, 1, "1")]);
        let mut d = po.draft();
        d.exchange_rate = Some(ExchangeRate::parse("1.1").unwrap());
        po.update(d, None, at(1)).unwrap();
        po.order(Some(Currency::EUR), "S", None, at(1)).unwrap();
        assert_eq!(po.exchange_rate(), None);
    }

    #[test]
    fn extra_costs_are_in_the_orders_or_the_stores_currency() {
        let mut po = po(vec![line(10, 1, "1")]);
        let mut d = po.draft();
        d.exchange_rate = Some(ExchangeRate::parse("1.1").unwrap());
        d.costs = vec![CostDraft {
            kind: CostKind::Duties,
            description: None,
            amount: Money::new(500, Currency::RSD).unwrap(),
            allocation: CostAllocation::ByValue,
        }];
        po.update(d, None, at(1)).unwrap();
        assert_eq!(
            po.order(Some(Currency::USD), "S", None, at(1)),
            Err(PurchasingError::ExtraCostCurrency {
                order: Currency::EUR,
                store: Currency::USD,
                got: Currency::RSD,
            })
        );
    }

    #[test]
    fn ordering_keeps_the_suppliers_name_and_sets_the_expected_date() {
        let mut po = po(vec![line(10, 1, "1")]);
        po.take_events();
        po.order(Some(Currency::EUR), "Spore Supply", Some(14), at(100))
            .unwrap();
        assert_eq!(po.status(), PurchaseOrderStatus::Ordered);
        assert_eq!(po.supplier_name(), Some("Spore Supply"));
        assert_eq!(po.ordered_at(), Some(at(100)));
        assert_eq!(po.expected_at(), Some(at(100 + 14 * 86_400)));
        assert_eq!(event_types(&mut po), ["purchase_order.ordered"]);
        assert_eq!(
            po.order(Some(Currency::EUR), "S", None, at(1)),
            Err(PurchasingError::NotDraft { status: "ordered" })
        );
    }

    #[test]
    fn an_expected_date_given_is_kept() {
        let mut po = po(vec![line(10, 1, "1")]);
        let mut d = po.draft();
        d.expected_at = Some(at(5));
        po.update(d, None, at(1)).unwrap();
        po.order(Some(Currency::EUR), "S", Some(14), at(100))
            .unwrap();
        assert_eq!(po.expected_at(), Some(at(5)));
    }

    #[test]
    fn a_draft_changes_freely_and_records_what_changed() {
        let mut po = po(vec![line(10, 1, "1")]);
        po.take_events();
        let mut d = po.draft();
        d.currency = Currency::USD;
        d.lines = vec![LineDraft {
            unit_cost: UnitCost::parse("2", Currency::USD).unwrap(),
            ..line(11, 3, "2")
        }];
        d.note = Some("rush".into());
        po.update(d, None, at(1)).unwrap();
        assert_eq!(po.revision(), 0, "drafts don't count revisions");
        let events = po.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "purchase_order.updated");
        assert_eq!(
            events[0].data["fields"],
            serde_json::json!(["currency", "note", "lines"])
        );
    }

    #[test]
    fn an_update_that_changes_nothing_records_nothing() {
        let mut po = po(vec![line(10, 1, "1")]);
        po.take_events();
        po.update(po.draft(), None, at(1)).unwrap();
        assert!(!po.is_changed());
    }

    #[test]
    fn an_ordered_order_keeps_its_currency_and_counts_revisions() {
        let mut po = ordered(vec![line(10, 5, "100")]);
        let mut d = po.draft();
        d.currency = Currency::USD;
        d.lines.clear();
        assert_eq!(
            po.update(d, Some(Currency::EUR), at(2)),
            Err(PurchasingError::CurrencyFixed)
        );
        let mut d = po.draft();
        d.lines[0].quantity = 8;
        po.update(d, Some(Currency::EUR), at(2)).unwrap();
        assert_eq!(po.revision(), 1);
        let events = po.take_events();
        assert_eq!(events[0].data["revision"], serde_json::json!(1));
    }

    #[test]
    fn an_ordered_order_keeps_what_arrived() {
        let mut po = ordered(vec![line(10, 5, "100"), line(11, 5, "100")]);
        po.record_receipt(&[got(10, 3, 1)], false, at(3)).unwrap();
        let mut d = po.draft();
        d.lines[0].quantity = 3;
        assert_eq!(
            po.update(d, Some(Currency::EUR), at(4)),
            Err(PurchasingError::BelowReceived {
                variant: variant(10),
                minimum: 4
            })
        );
        let mut d = po.draft();
        d.lines.remove(0);
        assert_eq!(
            po.update(d, Some(Currency::EUR), at(4)),
            Err(PurchasingError::LineReceived(variant(10)))
        );
        // A line nothing arrived on may go.
        let mut d = po.draft();
        d.lines.remove(1);
        po.update(d, Some(Currency::EUR), at(4)).unwrap();
        assert_eq!(po.lines().len(), 1);
        assert_eq!(po.lines()[0].received(), 3);
        assert_eq!(po.lines()[0].rejected(), 1);
    }

    #[test]
    fn lowering_an_ordered_line_to_what_arrived_closes_the_order() {
        let mut po = ordered(vec![line(10, 5, "100")]);
        po.record_receipt(&[got(10, 3, 0)], false, at(3)).unwrap();
        let mut d = po.draft();
        d.lines[0].quantity = 3;
        po.update(d, Some(Currency::EUR), at(4)).unwrap();
        assert_eq!(po.status(), PurchaseOrderStatus::Closed);
        assert_eq!(po.receipt_status(), ReceiptStatus::Received);
    }

    #[test]
    fn closed_and_cancelled_orders_dont_change() {
        let mut po = ordered(vec![line(10, 5, "100")]);
        po.close(at(5)).unwrap();
        let mut noted = po.draft();
        noted.note = Some("late".into());
        assert_eq!(
            po.update(noted, Some(Currency::EUR), at(6)),
            Err(PurchasingError::NotOpen { status: "closed" })
        );
        assert_eq!(
            po.cancel(at(6)),
            Err(PurchasingError::NotOpen { status: "closed" })
        );
    }

    #[test]
    fn closing_cancels_what_is_outstanding() {
        let mut po = ordered(vec![line(10, 5, "100"), line(11, 2, "100")]);
        po.record_receipt(&[got(10, 2, 1)], false, at(3)).unwrap();
        po.take_events();
        po.close(at(5)).unwrap();
        assert_eq!(po.status(), PurchaseOrderStatus::Closed);
        assert_eq!(po.closed_at(), Some(at(5)));
        assert_eq!(po.lines()[0].cancelled(), 2);
        assert_eq!(po.lines()[1].cancelled(), 2);
        assert_eq!(po.lines()[0].outstanding(), 0);
        assert_eq!(
            po.receipt_status(),
            ReceiptStatus::Partial,
            "closing isn't receiving"
        );
        assert_eq!(event_types(&mut po), ["purchase_order.closed"]);
    }

    #[test]
    fn only_ordered_orders_close() {
        let mut draft = po(vec![line(10, 1, "1")]);
        assert_eq!(
            draft.close(at(1)),
            Err(PurchasingError::NotOrdered { status: "draft" })
        );
    }

    #[test]
    fn cancelling_is_refused_once_goods_arrived() {
        let mut po = ordered(vec![line(10, 5, "100")]);
        po.record_receipt(&[got(10, 0, 1)], false, at(3)).unwrap();
        assert_eq!(po.cancel(at(4)), Err(PurchasingError::AlreadyReceived));
    }

    #[test]
    fn drafts_and_ordered_orders_cancel() {
        let mut draft = po(vec![line(10, 1, "1")]);
        draft.take_events();
        draft.cancel(at(2)).unwrap();
        assert_eq!(draft.status(), PurchaseOrderStatus::Cancelled);
        assert_eq!(draft.cancelled_at(), Some(at(2)));
        assert_eq!(event_types(&mut draft), ["purchase_order.cancelled"]);
        let mut ordered = ordered(vec![line(10, 1, "1")]);
        assert_eq!(ordered.cancel(at(2)), Ok(()));
    }

    #[test]
    fn only_drafts_are_deleted() {
        assert_eq!(po(vec![]).check_deletable(), Ok(()));
        assert_eq!(
            ordered(vec![line(10, 1, "1")]).check_deletable(),
            Err(PurchasingError::NotDraft { status: "ordered" })
        );
    }

    #[test]
    fn a_receipt_counts_accepted_and_rejected_units() {
        let mut po = ordered(vec![line(10, 5, "100"), line(11, 2, "100")]);
        po.record_receipt(&[got(10, 3, 1)], false, at(3)).unwrap();
        let line = po.line(variant(10)).unwrap();
        assert_eq!(
            (line.received(), line.rejected(), line.outstanding()),
            (3, 1, 1)
        );
        assert_eq!(po.receipt_status(), ReceiptStatus::Partial);
        assert_eq!(po.status(), PurchaseOrderStatus::Ordered);
        let events = po.take_events();
        assert_eq!(
            events[0].event_type,
            "purchase_order.receipt_status_changed"
        );
        assert_eq!(events[0].data["from"], serde_json::json!("none"));
        assert_eq!(events[0].data["to"], serde_json::json!("partial"));
    }

    #[test]
    fn the_last_line_accounted_for_closes_the_order() {
        let mut po = ordered(vec![line(10, 2, "100")]);
        po.record_receipt(&[got(10, 1, 1)], false, at(3)).unwrap();
        assert_eq!(po.status(), PurchaseOrderStatus::Closed);
        assert_eq!(po.receipt_status(), ReceiptStatus::Received);
        assert_eq!(
            event_types(&mut po),
            [
                "purchase_order.receipt_status_changed",
                "purchase_order.closed"
            ]
        );
    }

    #[test]
    fn receiving_more_than_is_outstanding_is_refused_with_each_lines_outstanding() {
        let mut po = ordered(vec![line(10, 2, "100"), line(11, 2, "100")]);
        assert_eq!(
            po.record_receipt(&[got(10, 3, 0), got(11, 2, 1)], false, at(3)),
            Err(PurchasingError::OverReceived(vec![
                Outstanding {
                    variant: variant(10),
                    outstanding: 2
                },
                Outstanding {
                    variant: variant(11),
                    outstanding: 2
                },
            ]))
        );
        assert_eq!(po.lines()[0].received(), 0, "nothing counted");
        po.record_receipt(&[got(10, 3, 0)], true, at(3)).unwrap();
        assert_eq!(po.lines()[0].received(), 3);
        assert_eq!(po.lines()[0].outstanding(), 0);
    }

    #[test]
    fn a_receipt_names_lines_on_the_order_once_with_some_units() {
        let mut po = ordered(vec![line(10, 2, "100")]);
        assert_eq!(
            po.record_receipt(&[got(99, 1, 0)], false, at(3)),
            Err(PurchasingError::UnknownLine(variant(99)))
        );
        assert_eq!(
            po.record_receipt(&[got(10, 1, 0), got(10, 1, 0)], false, at(3)),
            Err(PurchasingError::DuplicateLine(variant(10)))
        );
        assert_eq!(
            po.record_receipt(&[got(10, 0, 0)], false, at(3)),
            Err(PurchasingError::EmptyReceiptLine(variant(10)))
        );
    }

    #[test]
    fn only_ordered_orders_receive() {
        let mut draft = po(vec![line(10, 2, "100")]);
        assert_eq!(
            draft.record_receipt(&[got(10, 1, 0)], false, at(3)),
            Err(PurchasingError::NotOrdered { status: "draft" })
        );
    }

    #[test]
    fn taking_a_receipt_back_reopens_a_closed_order() {
        let mut po = ordered(vec![line(10, 2, "100")]);
        po.record_receipt(&[got(10, 2, 0)], false, at(3)).unwrap();
        po.take_events();
        po.take_back_receipt(&[got(10, 2, 0)]).unwrap();
        assert_eq!(po.status(), PurchaseOrderStatus::Ordered);
        assert_eq!(po.closed_at(), None);
        assert_eq!(po.receipt_status(), ReceiptStatus::None);
        assert_eq!(po.lines()[0].outstanding(), 2);
        let events = po.take_events();
        assert_eq!(events[0].event_type, "purchase_order.updated");
        assert_eq!(events[0].data["fields"], serde_json::json!(["status"]));
        assert_eq!(
            events[1].event_type,
            "purchase_order.receipt_status_changed"
        );
    }

    #[test]
    fn taking_back_more_than_a_line_counts_is_refused() {
        let mut po = ordered(vec![line(10, 2, "100")]);
        po.record_receipt(&[got(10, 1, 0)], false, at(3)).unwrap();
        assert_eq!(
            po.take_back_receipt(&[got(10, 1, 1)]),
            Err(PurchasingError::TakeBackMismatch(variant(10)))
        );
        assert_eq!(po.lines()[0].received(), 1, "all or nothing");
        let mut cancelled = self::po(vec![line(10, 1, "1")]);
        cancelled.cancel(at(1)).unwrap();
        assert_eq!(
            cancelled.take_back_receipt(&[got(10, 1, 0)]),
            Err(PurchasingError::NotOpen {
                status: "cancelled"
            })
        );
    }

    #[test]
    fn totals_add_lines_costs_and_tax() {
        // In a USD store the order needs a rate, and duties may be paid in dollars.
        let mut po = po(vec![line(10, 3, "100.5"), line(11, 2, "0.35")]);
        let mut d = po.draft();
        d.exchange_rate = Some(ExchangeRate::parse("1.1").unwrap());
        d.costs = vec![
            CostDraft {
                kind: CostKind::Shipping,
                description: None,
                amount: Money::new(500, Currency::EUR).unwrap(),
                allocation: CostAllocation::ByValue,
            },
            CostDraft {
                kind: CostKind::Duties,
                description: None,
                amount: Money::new(70, Currency::USD).unwrap(),
                allocation: CostAllocation::ByValue,
            },
        ];
        d.tax = Some(Money::new(60, Currency::EUR).unwrap());
        po.update(d, None, at(2)).unwrap();
        let totals = po.totals(Some(Currency::USD)).unwrap();
        // 3 × 100.5 = 301.5 → 302; 2 × 0.35 = 0.7 → 1.
        assert_eq!(totals.subtotal.amount(), 303);
        assert_eq!(totals.costs.amount(), 500);
        assert_eq!(totals.tax.amount(), 60);
        assert_eq!(totals.total.amount(), 863);
        assert_eq!(
            totals.costs_in_store_currency,
            Some(Money::new(70, Currency::USD).unwrap())
        );
    }

    #[test]
    fn landed_costs_spread_the_extra_costs_and_convert() {
        let mut po = po(vec![line(10, 10, "100"), line(11, 10, "300")]);
        let mut d = po.draft();
        d.costs = vec![CostDraft {
            kind: CostKind::Shipping,
            description: None,
            amount: Money::new(1000, Currency::EUR).unwrap(),
            allocation: CostAllocation::ByValue,
        }];
        po.update(d, None, at(1)).unwrap();
        assert_eq!(
            po.landed_costs(Currency::EUR, None).unwrap(),
            vec![Some(eur("125")), Some(eur("375"))]
        );
        let rate = ExchangeRate::parse("2").unwrap();
        let usd = |m: &str| UnitCost::parse(m, Currency::USD).unwrap();
        assert_eq!(
            po.landed_costs(Currency::USD, Some(rate)).unwrap(),
            vec![Some(usd("250")), Some(usd("750"))]
        );
        assert_eq!(
            po.landed_costs(Currency::USD, None).unwrap(),
            vec![None, None]
        );
    }

    #[test]
    fn totals_overflow_instead_of_panicking() {
        let most = UnitCost::new(UnitCost::MAX_SCALED, Currency::EUR).unwrap();
        let po = po(vec![LineDraft {
            unit_cost: most,
            ..line(10, MAX_QUANTITY, "1")
        }]);
        assert_eq!(
            po.totals(None),
            Err(PurchasingError::Money(MoneyError::Overflow))
        );
    }

    #[test]
    fn a_line_holds_at_most_max_quantity_units() {
        let too_many = PurchaseOrder::new(
            PurchaseOrderId::from_uuid(id(1)),
            PurchaseOrderNumber::new(1001).unwrap(),
            SupplierId::from_uuid(id(2)),
            draft(vec![line(10, MAX_QUANTITY + 1, "1")]),
        );
        assert_eq!(
            too_many.map(|_| ()),
            Err(PurchasingError::QuantityTooLarge {
                variant: variant(10),
                max: MAX_QUANTITY
            })
        );
        // Received counts stop there too, however far over a receipt is allowed.
        let mut po = ordered(vec![line(10, MAX_QUANTITY, "1")]);
        po.record_receipt(&[got(10, MAX_QUANTITY - 1, 0)], false, at(3))
            .unwrap();
        assert_eq!(
            po.record_receipt(&[got(10, 2, 0)], true, at(4)),
            Err(PurchasingError::QuantityTooLarge {
                variant: variant(10),
                max: MAX_QUANTITY
            })
        );
    }

    #[test]
    fn a_line_received_over_its_quantity_still_takes_other_edits() {
        let mut po = ordered(vec![line(10, 3, "100")]);
        po.record_receipt(&[got(10, 5, 0)], true, at(3)).unwrap();
        // Closed by the receipt, its extra costs still change.
        let mut d = po.draft();
        d.costs = vec![CostDraft {
            kind: CostKind::Shipping,
            description: None,
            amount: Money::new(300, Currency::EUR).unwrap(),
            allocation: CostAllocation::ByQuantity,
        }];
        assert_eq!(po.update(d, Some(Currency::EUR), at(4)), Ok(()));
        let mut d = po.draft();
        d.lines[0].quantity = 4;
        assert_eq!(
            po.update(d, Some(Currency::EUR), at(5)),
            Err(PurchasingError::BelowReceived {
                variant: variant(10),
                minimum: 5
            })
        );
    }

    #[test]
    fn extra_costs_reach_only_the_units_a_line_spreads_them_over() {
        let mut po = ordered(vec![line(10, 3, "100")]);
        assert_eq!(po.extra_bearing(variant(10), 2), 2);
        po.record_receipt(&[got(10, 2, 0)], false, at(3)).unwrap();
        // One of 3 left to carry them, whatever arrives.
        assert_eq!(po.extra_bearing(variant(10), 4), 1);
        let receipts = [
            ReceiptBasis {
                exchange_rate: None,
                lines: vec![got(10, 2, 0)],
            },
            ReceiptBasis {
                exchange_rate: None,
                lines: vec![got(10, 4, 0)],
            },
        ];
        let mut d = po.draft();
        d.costs = vec![CostDraft {
            kind: CostKind::Shipping,
            description: None,
            amount: Money::new(300, Currency::EUR).unwrap(),
            allocation: CostAllocation::ByQuantity,
        }];
        po.update(d, Some(Currency::EUR), at(4)).unwrap();
        // €1.00 a unit, on 3 units of the 6 that came.
        assert_eq!(
            po.extra_due(Currency::EUR, &receipts).unwrap()[&variant(10)],
            3 * 1_000_000
        );
    }

    #[test]
    fn restore_round_trips_the_snapshot() {
        let mut po = ordered(vec![line(10, 5, "100"), line(11, 2, "1")]);
        po.record_receipt(&[got(10, 2, 1)], false, at(3)).unwrap();
        po.take_events();
        let back = PurchaseOrder::restore(po.snapshot()).unwrap();
        assert_eq!(back, po);
        assert_eq!(back.lines()[0].rejected(), 1);
    }

    #[test]
    fn statuses_read_back_by_name() {
        for status in PurchaseOrderStatus::ALL {
            assert_eq!(PurchaseOrderStatus::parse(status.as_str()), Some(status));
        }
        for status in ReceiptStatus::ALL {
            assert_eq!(ReceiptStatus::parse(status.as_str()), Some(status));
        }
        for kind in CostKind::ALL {
            assert_eq!(CostKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(PurchaseOrderStatus::parse("sent"), None);
    }

    fn email(s: &str) -> Email {
        Email::new(s).unwrap()
    }

    fn sending(to: &[&str], cc: &[&str], message: Option<&str>) -> Sending {
        Sending {
            to: to.iter().map(|e| email(e)).collect(),
            cc: cc.iter().map(|e| email(e)).collect(),
            message: message.map(str::to_string),
            ship_to: Some(ShipTo {
                name: "Warehouse".into(),
                address: None,
            }),
        }
    }

    #[test]
    fn sending_a_draft_orders_it_first() {
        let mut po = po(vec![line(10, 5, "100")]);
        po.take_events();
        po.send(
            sending(&["orders@spore.example"], &[], Some("  Rush please  ")),
            Some(Currency::EUR),
            "Spore Supply",
            None,
            at(200),
        )
        .unwrap();
        assert_eq!(
            (po.status(), po.ordered_at(), po.sent_at()),
            (PurchaseOrderStatus::Ordered, Some(at(200)), Some(at(200)))
        );
        let events = po.take_events();
        let types: Vec<_> = events.iter().map(|e| e.event_type.to_string()).collect();
        assert_eq!(types, ["purchase_order.ordered", "purchase_order.sent"]);
        assert_eq!(
            events[1].data,
            serde_json::json!({
                "purchase_order": po.id(),
                "supplier": po.supplier(),
                "revision": 0,
                "to": ["orders@spore.example"],
                "cc": [],
                "message": "Rush please",
                "ship_to": {"name": "Warehouse", "address": null}
            })
        );
    }

    #[test]
    fn sending_an_ordered_order_again_names_its_revision() {
        let mut po = ordered(vec![line(10, 5, "100")]);
        let mut d = po.draft();
        d.lines[0].quantity = 8;
        po.update(d, Some(Currency::EUR), at(150)).unwrap();
        po.take_events();
        po.send(
            sending(&["a@spore.example"], &["b@spore.example"], None),
            Some(Currency::EUR),
            "Spore Supply",
            None,
            at(300),
        )
        .unwrap();
        let events = po.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data["revision"], 1);
        assert_eq!(events[0].data["cc"], serde_json::json!(["b@spore.example"]));
        assert_eq!(po.sent_at(), Some(at(300)));
        assert_eq!(po.sent_revision(), Some(1));
        // An edit after sending leaves the supplier an older revision.
        let mut d = po.draft();
        d.lines[0].quantity = 9;
        po.update(d, Some(Currency::EUR), at(400)).unwrap();
        assert_eq!((po.revision(), po.sent_revision()), (2, Some(1)));
    }

    #[test]
    fn a_send_needs_an_address_and_holds_at_most_ten() {
        let mut po = ordered(vec![line(10, 5, "100")]);
        let send = |po: &mut PurchaseOrder, s: Sending| {
            po.send(s, Some(Currency::EUR), "Spore Supply", None, at(300))
        };
        assert_eq!(
            send(&mut po, sending(&[], &["b@x.example"], None)),
            Err(PurchasingError::NoRecipients)
        );
        let many: Vec<String> = (0..11).map(|n| format!("p{n}@x.example")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(
            send(&mut po, sending(&many[..1], &many[1..], None)),
            Err(PurchasingError::TooManyRecipients { max: 10 })
        );
        // Repeats count once: a cc already in `to` is dropped.
        assert_eq!(
            send(
                &mut po,
                sending(&many[..10], &["p0@x.example", "p1@x.example"], None)
            ),
            Ok(())
        );
        assert_eq!(po.take_events()[0].data["cc"], serde_json::json!([]));
        let long = "é".repeat(2001);
        assert_eq!(
            send(&mut po, sending(&["a@x.example"], &[], Some(&long))),
            Err(PurchasingError::MessageTooLong { max: 2000 })
        );
    }

    #[test]
    fn a_closed_or_cancelled_order_isnt_sent_and_nothing_changes() {
        let mut po = ordered(vec![line(10, 5, "100")]);
        po.close(at(150)).unwrap();
        po.take_events();
        assert_eq!(
            po.send(
                sending(&["a@x.example"], &[], None),
                Some(Currency::EUR),
                "Spore Supply",
                None,
                at(300)
            ),
            Err(PurchasingError::NotOpen { status: "closed" })
        );
        assert_eq!(po.sent_at(), None);
        assert!(po.take_events().is_empty());
        // A draft that can't be ordered isn't sent either.
        let mut draft = po_without_destination();
        assert_eq!(
            draft.send(
                sending(&["a@x.example"], &[], None),
                Some(Currency::EUR),
                "Spore Supply",
                None,
                at(300)
            ),
            Err(PurchasingError::DestinationRequired)
        );
        assert_eq!(
            (draft.status(), draft.sent_at()),
            (PurchaseOrderStatus::Draft, None)
        );
    }

    fn po_without_destination() -> PurchaseOrder {
        PurchaseOrder::new(
            PurchaseOrderId::from_uuid(id(1)),
            PurchaseOrderNumber::new(1001).unwrap(),
            SupplierId::from_uuid(id(2)),
            PurchaseOrderDraft {
                lines: vec![line(10, 1, "1")],
                ..PurchaseOrderDraft::new(Currency::EUR)
            },
        )
        .unwrap()
    }

    // ---- late costs ----

    fn shipping(amount: i64, currency: Currency) -> CostDraft {
        CostDraft {
            kind: CostKind::Shipping,
            description: None,
            amount: Money::new(amount, currency).unwrap(),
            allocation: CostAllocation::ByQuantity,
        }
    }

    fn basis(rate: Option<ExchangeRate>, lines: Vec<ReceivedQuantities>) -> ReceiptBasis {
        ReceiptBasis {
            exchange_rate: rate,
            lines,
        }
    }

    fn with_costs(po: &PurchaseOrder, costs: Vec<CostDraft>) -> PurchaseOrderDraft {
        PurchaseOrderDraft {
            costs,
            ..po.draft()
        }
    }

    #[test]
    fn new_lines_start_with_no_extra_value() {
        let po = ordered(vec![line(10, 5, "100")]);
        assert_eq!(po.lines()[0].extra_applied(), Some(0));
    }

    #[test]
    fn a_closed_order_takes_changes_to_its_extra_costs_only() {
        let mut po = ordered(vec![line(10, 5, "100")]);
        po.close(at(5)).unwrap();
        po.take_events();
        let costs = with_costs(&po, vec![shipping(1000, Currency::EUR)]);
        assert_eq!(po.update(costs, Some(Currency::EUR), at(6)), Ok(()));
        assert_eq!(
            (po.status(), po.costs().len(), po.revision()),
            (PurchaseOrderStatus::Closed, 1, 1)
        );
        assert_eq!(event_types(&mut po), ["purchase_order.updated"]);
        let mut both = with_costs(&po, vec![]);
        both.note = Some("and a note".into());
        assert_eq!(
            po.update(both, Some(Currency::EUR), at(7)),
            Err(PurchasingError::NotOpen { status: "closed" })
        );
    }

    #[test]
    fn extras_per_unit_is_the_landed_cost_less_the_unit_cost_alone() {
        let mut po = ordered(vec![line(10, 4, "100"), line(11, 6, "300")]);
        po.update(
            with_costs(&po, vec![shipping(1000, Currency::EUR)]),
            Some(Currency::EUR),
            at(5),
        )
        .unwrap();
        let extras = po.extras_per_unit(Currency::EUR, None).unwrap();
        // €10.00 over 10 units by quantity: 100 cents, 1,000,000 scaled, each.
        assert_eq!(extras[&variant(10)], Some(1_000_000));
        assert_eq!(extras[&variant(11)], Some(1_000_000));
    }

    #[test]
    fn extras_need_a_rate_in_another_currency() {
        let po = ordered(vec![line(10, 4, "100")]);
        let extras = po.extras_per_unit(Currency::USD, None).unwrap();
        assert_eq!(extras[&variant(10)], None);
    }

    #[test]
    fn a_cost_added_after_a_receipt_revalues_what_arrived() {
        let mut po = ordered(vec![line(10, 10, "100")]);
        po.record_receipt(&[got(10, 4, 0)], false, at(5)).unwrap();
        po.add_extra_applied(variant(10), 4, 0).unwrap();
        let receipts = [basis(None, vec![got(10, 4, 0)])];
        let before = po.extra_due(Currency::EUR, &receipts).unwrap();
        po.update(
            with_costs(&po, vec![shipping(1000, Currency::EUR)]),
            Some(Currency::EUR),
            at(6),
        )
        .unwrap();
        // €10.00 over the 10 ordered: 100 cents a unit, 4 arrived: 400 cents.
        assert_eq!(
            po.revalue(Currency::EUR, &receipts, &before),
            Ok(vec![Revaluation {
                variant: variant(10),
                value: 4_000_000
            }])
        );
        assert_eq!(po.lines()[0].extra_applied(), Some(4_000_000));
        // Nothing changed since: nothing more to apply.
        assert_eq!(po.revalue(Currency::EUR, &receipts, &before), Ok(vec![]));
    }

    #[test]
    fn a_cost_lowered_or_removed_takes_value_back() {
        let mut po = ordered(vec![line(10, 10, "100")]);
        po.update(
            with_costs(&po, vec![shipping(1000, Currency::EUR)]),
            Some(Currency::EUR),
            at(4),
        )
        .unwrap();
        po.record_receipt(&[got(10, 4, 0)], false, at(5)).unwrap();
        po.add_extra_applied(variant(10), 4, 1_000_000).unwrap();
        let receipts = [basis(None, vec![got(10, 4, 0)])];
        po.update(
            with_costs(&po, vec![shipping(500, Currency::EUR)]),
            Some(Currency::EUR),
            at(6),
        )
        .unwrap();
        assert_eq!(
            po.revalue(Currency::EUR, &receipts, &HashMap::new()),
            Ok(vec![Revaluation {
                variant: variant(10),
                value: -2_000_000
            }])
        );
        po.update(with_costs(&po, vec![]), Some(Currency::EUR), at(7))
            .unwrap();
        assert_eq!(
            po.revalue(Currency::EUR, &receipts, &HashMap::new()),
            Ok(vec![Revaluation {
                variant: variant(10),
                value: -2_000_000
            }])
        );
        assert_eq!(po.lines()[0].extra_applied(), Some(0));
    }

    #[test]
    fn each_receipt_counts_at_its_own_rate() {
        // An order in USD for a EUR store: a €10.00 duty is in the store's currency and
        // needs no rate; $10.00 shipping converts at each receipt's rate.
        let mut po = PurchaseOrder::new(
            PurchaseOrderId::from_uuid(id(1)),
            PurchaseOrderNumber::new(1001).unwrap(),
            SupplierId::from_uuid(id(2)),
            PurchaseOrderDraft {
                destination: Some(LocationId::from_uuid(id(50))),
                exchange_rate: Some(ExchangeRate::parse("0.5").unwrap()),
                lines: vec![LineDraft {
                    unit_cost: UnitCost::parse("100", Currency::USD).unwrap(),
                    ..line(10, 10, "100")
                }],
                ..PurchaseOrderDraft::new(Currency::USD)
            },
        )
        .unwrap();
        po.order(Some(Currency::EUR), "Spore Supply", None, at(1))
            .unwrap();
        po.record_receipt(&[got(10, 2, 0)], false, at(2)).unwrap();
        po.record_receipt(&[got(10, 2, 0)], false, at(3)).unwrap();
        let receipts = [
            basis(
                Some(ExchangeRate::parse("0.5").unwrap()),
                vec![got(10, 2, 0)],
            ),
            basis(Some(ExchangeRate::parse("1").unwrap()), vec![got(10, 2, 0)]),
        ];
        po.update(
            with_costs(
                &po,
                vec![shipping(1000, Currency::USD), shipping(1000, Currency::EUR)],
            ),
            Some(Currency::EUR),
            at(4),
        )
        .unwrap();
        // Per unit: $1.00 shipping (€0.50 at 0.5, €1.00 at 1) and €1.00 duty.
        // 2 × (50 + 100) + 2 × (100 + 100) = 700 cents.
        assert_eq!(
            po.revalue(Currency::EUR, &receipts, &HashMap::new()),
            Ok(vec![Revaluation {
                variant: variant(10),
                value: 7_000_000
            }])
        );
    }

    #[test]
    fn closing_early_spreads_the_costs_over_fewer_units() {
        let mut po = ordered(vec![line(10, 10, "100")]);
        po.update(
            with_costs(&po, vec![shipping(1000, Currency::EUR)]),
            Some(Currency::EUR),
            at(4),
        )
        .unwrap();
        po.record_receipt(&[got(10, 5, 0)], false, at(5)).unwrap();
        po.add_extra_applied(variant(10), 5, 1_000_000).unwrap();
        po.close(at(6)).unwrap();
        // 10 euros over the 5 that came: 200 cents each, 100 more for 5 units.
        let receipts = [basis(None, vec![got(10, 5, 0)])];
        assert_eq!(
            po.revalue(Currency::EUR, &receipts, &HashMap::new()),
            Ok(vec![Revaluation {
                variant: variant(10),
                value: 5_000_000
            }])
        );
    }

    #[test]
    fn a_line_without_a_kept_value_is_measured_against_before() {
        let mut snapshot = ordered(vec![line(10, 10, "100")]).snapshot();
        snapshot.lines[0].received = 4;
        snapshot.lines[0].extra_applied = None;
        let mut po = PurchaseOrder::restore(snapshot).unwrap();
        let receipts = [basis(None, vec![got(10, 4, 0)])];
        let before = po.extra_due(Currency::EUR, &receipts).unwrap();
        po.update(
            with_costs(&po, vec![shipping(1000, Currency::EUR)]),
            Some(Currency::EUR),
            at(6),
        )
        .unwrap();
        assert_eq!(
            po.revalue(Currency::EUR, &receipts, &before),
            Ok(vec![Revaluation {
                variant: variant(10),
                value: 4_000_000
            }])
        );
        // Without `before`, it's taken as right and only starts being kept.
        let mut snapshot = po.snapshot();
        snapshot.lines[0].extra_applied = None;
        let mut po = PurchaseOrder::restore(snapshot).unwrap();
        assert_eq!(
            po.revalue(Currency::EUR, &receipts, &HashMap::new()),
            Ok(vec![])
        );
        assert_eq!(po.lines()[0].extra_applied(), Some(4_000_000));
    }

    #[test]
    fn extra_value_is_added_and_taken_back_per_receipt() {
        let mut po = ordered(vec![line(10, 10, "100")]);
        po.record_receipt(&[got(10, 4, 0)], false, at(5)).unwrap();
        po.add_extra_applied(variant(10), 4, 250).unwrap();
        assert_eq!(po.lines()[0].extra_applied(), Some(1_000));
        po.take_extra_applied(variant(10), 4, 250).unwrap();
        assert_eq!(po.lines()[0].extra_applied(), Some(0));
        // A line that arrived before values were kept starts none until revalued.
        let mut snapshot = po.snapshot();
        snapshot.lines[0].extra_applied = None;
        let mut po = PurchaseOrder::restore(snapshot).unwrap();
        po.record_receipt(&[got(10, 2, 0)], false, at(6)).unwrap();
        po.add_extra_applied(variant(10), 2, 250).unwrap();
        assert_eq!(po.lines()[0].extra_applied(), None);
    }

    #[test]
    fn the_kept_value_survives_storage() {
        let mut po = ordered(vec![line(10, 10, "100")]);
        po.record_receipt(&[got(10, 4, 0)], false, at(5)).unwrap();
        po.add_extra_applied(variant(10), 4, -3).unwrap();
        let restored = PurchaseOrder::restore(po.snapshot()).unwrap();
        assert_eq!(restored.lines()[0].extra_applied(), Some(-12));
    }
}
