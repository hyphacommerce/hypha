use hypha_core::{
    AddressParts, AuthError, Ctx, Currency, ErrorCode, IdGenerator, Page, PageRequest, RepoError,
    Scope, SupplierId, VariantId,
};
use thiserror::Error;

use crate::{
    ContactDraft, PaymentTermsDraft, PurchasingError, Supplier, SupplierDraft, SupplierVariant,
    SupplierVariantDraft,
    repo::{
        HasSupplierVariants, HasSuppliers, SupplierFilter, SupplierRepo, SupplierVariantRepo, keys,
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
