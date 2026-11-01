use hypha_core::{
    AddressParts, AuthError, Clock, Ctx, Currency, ErrorCode, ExchangeRate, IdGenerator, Keyset,
    LocationId, Money, Page, PageRequest, PurchaseOrderId, RepoError, Scope, SupplierId, Timestamp,
    UnitCost, VariantId,
};
use thiserror::Error;

use crate::{
    ContactDraft, CostDraft, LineDraft, PaymentTermsDraft, PurchaseOrder, PurchaseOrderDraft,
    PurchaseOrderSummary, PurchasingError, Supplier, SupplierDraft, SupplierVariant,
    SupplierVariantDraft,
    repo::{
        HasPurchaseOrders, HasSupplierVariants, HasSuppliers, PurchaseOrderFilter,
        PurchaseOrderRepo, PurchaseOrderSort, SupplierFilter, SupplierRepo, SupplierVariantRepo,
        keys,
    },
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Purchasing(#[from] PurchasingError),
    #[error("supplier {0} not found")]
    SupplierNotFound(SupplierId),
    #[error("supplier {supplier} doesn't list variant {variant}")]
    SupplierVariantNotFound {
        supplier: SupplierId,
        variant: VariantId,
    },
    /// Its catalogue has costs in the old currency: they'd be read in the new one.
    #[error("supplier {0}'s catalogue has costs in its currency: clear them first")]
    SupplierCurrencyInUse(SupplierId),
    /// A purchase order names it; archive it instead.
    #[error("supplier {0} is on purchase orders: archive it instead")]
    SupplierInUse(SupplierId),
    #[error("supplier {0} is archived: it takes no new purchase orders")]
    SupplierArchived(SupplierId),
    #[error("purchase order {0} not found")]
    PurchaseOrderNotFound(PurchaseOrderId),
    /// A line names a variant the store doesn't have.
    #[error("variant {0} not found")]
    UnknownVariant(VariantId),
    /// No cost was given, and neither the supplier's catalogue nor an earlier order has
    /// one in the order's currency.
    #[error("variant {0}'s line needs a unit cost")]
    UnitCostRequired(VariantId),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Purchasing(e) => e.code(),
            CommandError::SupplierNotFound(_) => "supplier_not_found",
            CommandError::SupplierVariantNotFound { .. } => "supplier_variant_not_found",
            CommandError::SupplierCurrencyInUse(_) => "supplier_currency_in_use",
            CommandError::SupplierInUse(_) => "supplier_in_use",
            CommandError::SupplierArchived(_) => "supplier_archived",
            CommandError::PurchaseOrderNotFound(_) => "purchase_order_not_found",
            CommandError::UnknownVariant(_) => "unknown_variant",
            CommandError::UnitCostRequired(_) => "unit_cost_required",
        }
    }
}

pub async fn create_supplier<S: HasSuppliers>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    draft: SupplierDraft,
) -> Result<Supplier, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let mut supplier = Supplier::new(SupplierId::from_uuid(ids.new_uuid()), draft)?;
    ports.suppliers().insert(&mut supplier).await?;
    Ok(supplier)
}

pub async fn get_supplier<S: HasSuppliers>(
    ctx: &Ctx,
    ports: &S,
    id: SupplierId,
) -> Result<Supplier, CommandError> {
    ctx.require(Scope::PurchasingRead)?;
    load(ports, id).await
}

async fn load<S: HasSuppliers>(ports: &S, id: SupplierId) -> Result<Supplier, CommandError> {
    ports
        .suppliers()
        .get(id)
        .await?
        .ok_or(CommandError::SupplierNotFound(id))
}

/// A page of suppliers, newest first.
pub async fn list_suppliers<S: HasSuppliers>(
    ctx: &Ctx,
    ports: &S,
    filter: SupplierFilter,
    page: PageRequest<SupplierId>,
) -> Result<Page<Supplier>, CommandError> {
    ctx.require(Scope::PurchasingRead)?;
    Ok(ports.suppliers().list(&filter, page).await?)
}

/// What to change on a supplier; `None` keeps a field, `Some(None)` clears an optional
/// one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateSupplier {
    pub name: Option<String>,
    /// Replaces them all.
    pub contacts: Option<Vec<ContactDraft>>,
    pub address: Option<Option<AddressParts>>,
    pub currency: Option<Currency>,
    pub payment_terms: Option<Option<PaymentTermsDraft>>,
    pub lead_time_days: Option<Option<u32>>,
    pub tax_number: Option<Option<String>>,
    pub notes: Option<Option<String>>,
    pub archived: Option<bool>,
}

/// Changes a supplier, checked as a whole. Its currency stays while its catalogue has
/// costs (`supplier_currency_in_use`). Nothing changed writes nothing.
pub async fn update_supplier<S: HasSuppliers + HasSupplierVariants>(
    ctx: &Ctx,
    ports: &S,
    id: SupplierId,
    input: UpdateSupplier,
) -> Result<Supplier, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let mut supplier = load(ports, id).await?;
    let mut draft = supplier.draft();
    if let Some(name) = input.name {
        draft.name = name;
    }
    if let Some(contacts) = input.contacts {
        draft.contacts = contacts;
    }
    if let Some(address) = input.address {
        draft.address = address;
    }
    if let Some(currency) = input.currency {
        draft.currency = currency;
    }
    if let Some(terms) = input.payment_terms {
        draft.payment_terms = terms;
    }
    if let Some(days) = input.lead_time_days {
        draft.lead_time_days = days;
    }
    if let Some(tax_number) = input.tax_number {
        draft.tax_number = tax_number;
    }
    if let Some(notes) = input.notes {
        draft.notes = notes;
    }
    if let Some(archived) = input.archived {
        draft.archived = archived;
    }
    // A row whose cost is set at this moment can slip past the check; its currency is
    // checked against the supplier as the row was written.
    if draft.currency != supplier.currency() && ports.supplier_variants().has_costs(id).await? {
        return Err(CommandError::SupplierCurrencyInUse(id));
    }
    supplier.update(draft)?;
    if supplier.is_changed() {
        ports.suppliers().update(&mut supplier).await?;
    }
    Ok(supplier)
}

/// Deletes a supplier with its catalogue. Refused while a purchase order names it
/// (`supplier_in_use`): archive it instead.
pub async fn delete_supplier<S: HasSuppliers>(
    ctx: &Ctx,
    ports: &S,
    id: SupplierId,
) -> Result<(), CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let mut supplier = load(ports, id).await?;
    supplier.delete();
    match ports.suppliers().delete(&mut supplier).await {
        Err(RepoError::Reference(keys::SUPPLIER_IN_USE)) => Err(CommandError::SupplierInUse(id)),
        other => Ok(other?),
    }
}

/// Sets what the supplier charges for `variant`, replacing the row it had. The caller
/// checks the variant exists: this crate can't see products.
pub async fn set_supplier_variant<S: HasSuppliers + HasSupplierVariants>(
    ctx: &Ctx,
    ports: &S,
    supplier: SupplierId,
    variant: VariantId,
    draft: SupplierVariantDraft,
) -> Result<SupplierVariant, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let found = load(ports, supplier).await?;
    let row = SupplierVariant::new(supplier, found.currency(), variant, draft)?;
    match ports.supplier_variants().put(&row).await {
        // Deleted since it was read.
        Err(RepoError::Reference(keys::CATALOGUE_SUPPLIER)) => {
            Err(CommandError::SupplierNotFound(supplier))
        }
        other => Ok(other.map(|()| row)?),
    }
}

pub async fn supplier_variant<S: HasSuppliers + HasSupplierVariants>(
    ctx: &Ctx,
    ports: &S,
    supplier: SupplierId,
    variant: VariantId,
) -> Result<SupplierVariant, CommandError> {
    ctx.require(Scope::PurchasingRead)?;
    load(ports, supplier).await?;
    ports
        .supplier_variants()
        .get(supplier, variant)
        .await?
        .ok_or(CommandError::SupplierVariantNotFound { supplier, variant })
}

/// Takes `variant` off the supplier's catalogue.
pub async fn remove_supplier_variant<S: HasSuppliers + HasSupplierVariants>(
    ctx: &Ctx,
    ports: &S,
    supplier: SupplierId,
    variant: VariantId,
) -> Result<(), CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    load(ports, supplier).await?;
    if !ports.supplier_variants().remove(supplier, variant).await? {
        return Err(CommandError::SupplierVariantNotFound { supplier, variant });
    }
    Ok(())
}

/// A page of the supplier's catalogue, newest variant first.
pub async fn list_supplier_variants<S: HasSuppliers + HasSupplierVariants>(
    ctx: &Ctx,
    ports: &S,
    supplier: SupplierId,
    page: PageRequest<VariantId>,
) -> Result<Page<SupplierVariant>, CommandError> {
    ctx.require(Scope::PurchasingRead)?;
    load(ports, supplier).await?;
    Ok(ports.supplier_variants().list(supplier, page).await?)
}

/// Who sells `variant`, and on what terms.
pub async fn variant_suppliers<S: HasSupplierVariants>(
    ctx: &Ctx,
    ports: &S,
    variant: VariantId,
) -> Result<Vec<SupplierVariant>, CommandError> {
    ctx.require(Scope::PurchasingRead)?;
    Ok(ports.supplier_variants().for_variant(variant).await?)
}

/// What the host knows about a variant a line names: the snapshot a line keeps, since the
/// variant may be removed later. This crate can't see products.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantInfo {
    pub variant: VariantId,
    pub product_title: String,
    pub variant_title: Option<String>,
    pub sku: Option<String>,
}

/// A line as asked for. An absent unit cost or supplier SKU starts from the line the
/// order already has, else the supplier's catalogue row, else the variant's line on the
/// supplier's latest order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineInput {
    pub variant: VariantId,
    pub quantity: u32,
    pub unit_cost: Option<UnitCost>,
    /// `Some(None)` clears it.
    pub supplier_sku: Option<Option<String>>,
}

/// A new purchase order. The currency and payment terms default to the supplier's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPurchaseOrder {
    pub supplier: SupplierId,
    pub destination: Option<LocationId>,
    pub currency: Option<Currency>,
    pub exchange_rate: Option<ExchangeRate>,
    /// `None` copies the supplier's; `Some(None)` sets none.
    pub payment_terms: Option<Option<PaymentTermsDraft>>,
    pub expected_at: Option<Timestamp>,
    pub supplier_reference: Option<String>,
    pub note_to_supplier: Option<String>,
    pub note: Option<String>,
    pub lines: Vec<LineInput>,
    pub costs: Vec<CostDraft>,
    pub tax: Option<Money>,
}

impl NewPurchaseOrder {
    /// An empty draft to `supplier`.
    pub fn to(supplier: SupplierId) -> Self {
        Self {
            supplier,
            destination: None,
            currency: None,
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

/// What to change on a purchase order; `None` keeps a field, `Some(None)` clears an
/// optional one. Lines and costs, when given, replace them all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdatePurchaseOrder {
    pub destination: Option<Option<LocationId>>,
    pub currency: Option<Currency>,
    pub exchange_rate: Option<Option<ExchangeRate>>,
    pub payment_terms: Option<Option<PaymentTermsDraft>>,
    pub expected_at: Option<Option<Timestamp>>,
    pub supplier_reference: Option<Option<String>>,
    pub note_to_supplier: Option<Option<String>>,
    pub note: Option<Option<String>>,
    pub lines: Option<Vec<LineInput>>,
    pub costs: Option<Vec<CostDraft>>,
    pub tax: Option<Option<Money>>,
}

/// Turns asked-for lines into drafts in `currency`, filling what's absent.
async fn resolve_lines<S: HasSupplierVariants + HasPurchaseOrders>(
    ports: &S,
    supplier: SupplierId,
    currency: Currency,
    existing: Option<&PurchaseOrder>,
    variants: &[VariantInfo],
    inputs: Vec<LineInput>,
) -> Result<Vec<LineDraft>, CommandError> {
    let mut lines = Vec::with_capacity(inputs.len());
    for input in inputs {
        let current = existing.and_then(|po| po.line(input.variant));
        let (mut sku, mut cost) = match current {
            Some(line) => (
                line.supplier_sku().map(str::to_string),
                Some(line.unit_cost()),
            ),
            None => (None, None),
        };
        if current.is_none() {
            if let Some(row) = ports
                .supplier_variants()
                .get(supplier, input.variant)
                .await?
            {
                sku = row.supplier_sku().map(str::to_string);
                cost = row.unit_cost();
            } else if let Some(last) = ports
                .purchase_orders()
                .last_line(supplier, input.variant)
                .await?
            {
                sku = last.supplier_sku().map(str::to_string);
                cost = Some(last.unit_cost());
            }
        }
        let cost = input
            .unit_cost
            .or(cost.filter(|c| c.currency() == currency))
            .ok_or(CommandError::UnitCostRequired(input.variant))?;
        if let Some(given) = input.supplier_sku {
            sku = given;
        }
        let draft = match current {
            Some(line) => LineDraft {
                supplier_sku: sku,
                quantity: input.quantity,
                unit_cost: cost,
                ..line.draft()
            },
            None => {
                let info = variants
                    .iter()
                    .find(|v| v.variant == input.variant)
                    .ok_or(CommandError::UnknownVariant(input.variant))?;
                LineDraft {
                    variant: input.variant,
                    product_title: info.product_title.clone(),
                    variant_title: info.variant_title.clone(),
                    sku: info.sku.clone(),
                    supplier_sku: sku,
                    quantity: input.quantity,
                    unit_cost: cost,
                }
            }
        };
        lines.push(draft);
    }
    Ok(lines)
}

/// Writes a draft purchase order to `supplier`, numbered from the store's sequence.
/// Archived suppliers take none (`supplier_archived`). `variants` describes the variants
/// the lines name.
pub async fn create_purchase_order<S: HasSuppliers + HasSupplierVariants + HasPurchaseOrders>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    variants: &[VariantInfo],
    input: NewPurchaseOrder,
) -> Result<PurchaseOrder, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let supplier = load(ports, input.supplier).await?;
    if supplier.archived() {
        return Err(CommandError::SupplierArchived(supplier.id()));
    }
    let currency = input.currency.unwrap_or(supplier.currency());
    let lines = resolve_lines(ports, supplier.id(), currency, None, variants, input.lines).await?;
    let payment_terms = match input.payment_terms {
        Some(terms) => terms,
        None => supplier.payment_terms().map(|t| PaymentTermsDraft {
            due: t.due(),
            note: t.note().map(str::to_string),
        }),
    };
    let draft = PurchaseOrderDraft {
        destination: input.destination,
        currency,
        exchange_rate: input.exchange_rate,
        payment_terms,
        expected_at: input.expected_at,
        supplier_reference: input.supplier_reference,
        note_to_supplier: input.note_to_supplier,
        note: input.note,
        lines,
        costs: input.costs,
        tax: input.tax,
    };
    let number = ports.purchase_orders().next_number().await?;
    let mut po = PurchaseOrder::new(
        PurchaseOrderId::from_uuid(ids.new_uuid()),
        number,
        supplier.id(),
        draft,
    )?;
    match ports.purchase_orders().insert(&mut po).await {
        // Deleted since it was read.
        Err(RepoError::Reference(keys::PURCHASE_ORDER_SUPPLIER)) => {
            Err(CommandError::SupplierNotFound(supplier.id()))
        }
        other => Ok(other.map(|()| po)?),
    }
}

pub async fn get_purchase_order<S: HasPurchaseOrders>(
    ctx: &Ctx,
    ports: &S,
    id: PurchaseOrderId,
) -> Result<PurchaseOrder, CommandError> {
    ctx.require(Scope::PurchasingRead)?;
    load_po(ports, id).await
}

async fn load_po<S: HasPurchaseOrders>(
    ports: &S,
    id: PurchaseOrderId,
) -> Result<PurchaseOrder, CommandError> {
    ports
        .purchase_orders()
        .get(id)
        .await?
        .ok_or(CommandError::PurchaseOrderNotFound(id))
}

/// A page of purchase orders, sorted `sort`. The caller passes a position whose key
/// fits the sort (`PurchaseOrderSort::fits`).
pub async fn list_purchase_orders<S: HasPurchaseOrders>(
    ctx: &Ctx,
    ports: &S,
    filter: PurchaseOrderFilter,
    sort: PurchaseOrderSort,
    page: PageRequest<Keyset<PurchaseOrderId>>,
) -> Result<Page<PurchaseOrderSummary>, CommandError> {
    ctx.require(Scope::PurchasingRead)?;
    Ok(ports.purchase_orders().list(&filter, sort, page).await?)
}

/// Changes a draft or ordered purchase order (`PurchaseOrder::update`'s rules). `store`
/// is the store's base currency.
pub async fn update_purchase_order<S: HasSupplierVariants + HasPurchaseOrders>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    store: Option<Currency>,
    variants: &[VariantInfo],
    id: PurchaseOrderId,
    input: UpdatePurchaseOrder,
) -> Result<PurchaseOrder, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let mut po = load_po(ports, id).await?;
    let mut draft = po.draft();
    if let Some(destination) = input.destination {
        draft.destination = destination;
    }
    if let Some(currency) = input.currency {
        draft.currency = currency;
    }
    if let Some(rate) = input.exchange_rate {
        draft.exchange_rate = rate;
    }
    if let Some(terms) = input.payment_terms {
        draft.payment_terms = terms;
    }
    if let Some(expected_at) = input.expected_at {
        draft.expected_at = expected_at;
    }
    if let Some(reference) = input.supplier_reference {
        draft.supplier_reference = reference;
    }
    if let Some(note) = input.note_to_supplier {
        draft.note_to_supplier = note;
    }
    if let Some(note) = input.note {
        draft.note = note;
    }
    if let Some(lines) = input.lines {
        draft.lines = resolve_lines(
            ports,
            po.supplier(),
            draft.currency,
            Some(&po),
            variants,
            lines,
        )
        .await?;
    } else if draft.currency != po.currency() {
        // Lines kept as they are would be in the old currency: they say so.
        for line in &mut draft.lines {
            if line.unit_cost.currency() != draft.currency {
                return Err(PurchasingError::LineCurrencyMismatch {
                    variant: line.variant,
                    expected: draft.currency,
                    got: line.unit_cost.currency(),
                }
                .into());
            }
        }
    }
    if let Some(costs) = input.costs {
        draft.costs = costs;
    }
    if let Some(tax) = input.tax {
        draft.tax = tax;
    }
    po.update(draft, store, clock.now())?;
    if po.is_changed() {
        ports.purchase_orders().update(&mut po).await?;
    }
    Ok(po)
}

/// Deletes a draft purchase order (`purchase_order_not_draft` otherwise).
pub async fn delete_purchase_order<S: HasPurchaseOrders>(
    ctx: &Ctx,
    ports: &S,
    id: PurchaseOrderId,
) -> Result<(), CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let mut po = load_po(ports, id).await?;
    po.check_deletable()?;
    ports.purchase_orders().delete(&mut po).await?;
    Ok(())
}

/// Places a draft with its supplier (`PurchaseOrder::order`'s rules), keeping the
/// supplier's name and using its lead time for an unset expected date. An archived
/// supplier takes no orders (`supplier_archived`).
pub async fn order_purchase_order<S: HasSuppliers + HasPurchaseOrders>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    store: Option<Currency>,
    id: PurchaseOrderId,
) -> Result<PurchaseOrder, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let mut po = load_po(ports, id).await?;
    let supplier = load(ports, po.supplier()).await?;
    if supplier.archived() {
        return Err(CommandError::SupplierArchived(supplier.id()));
    }
    po.order(
        store,
        supplier.name(),
        supplier.lead_time_days(),
        clock.now(),
    )?;
    ports.purchase_orders().update(&mut po).await?;
    Ok(po)
}

/// Ends an ordered purchase order early: what's outstanding is cancelled.
pub async fn close_purchase_order<S: HasPurchaseOrders>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: PurchaseOrderId,
) -> Result<PurchaseOrder, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let mut po = load_po(ports, id).await?;
    po.close(clock.now())?;
    ports.purchase_orders().update(&mut po).await?;
    Ok(po)
}

/// Calls off a draft or ordered purchase order nothing arrived on.
pub async fn cancel_purchase_order<S: HasPurchaseOrders>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: PurchaseOrderId,
) -> Result<PurchaseOrder, CommandError> {
    ctx.require(Scope::PurchasingWrite)?;
    let mut po = load_po(ports, id).await?;
    po.cancel(clock.now())?;
    ports.purchase_orders().update(&mut po).await?;
    Ok(po)
}
