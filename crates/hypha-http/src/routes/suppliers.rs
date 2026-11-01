//! Suppliers and their catalogues: who the store buys from, and what each sells it at
//! what price.

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{
    Ctx, Currency, Email, ProductId, Scope, Stamped, SupplierId, UnitCost, VariantId,
};
use hypha_products::{HasProducts, Product, ProductError, ProductRepo};
use hypha_purchasing::{
    ContactDraft, HasSuppliers, PaymentDue, PaymentTermsDraft, Supplier, SupplierDraft,
    SupplierFilter, SupplierRepo, SupplierVariant, SupplierVariantDraft, UpdateSupplier,
    create_supplier, delete_supplier, get_supplier, list_supplier_variants, list_suppliers,
    remove_supplier_variant, set_supplier_variant, update_supplier, variant_suppliers,
};
use serde::{Deserialize, Serialize};

use super::{
    address::{PostalAddressRequest, PostalAddressResponse},
    page::{ListQuery, PageParams, PageResponse},
    stamp_times,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

// ---------- suppliers ----------

/// Someone at a supplier.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SupplierContactResponse {
    name: String,
    email: Option<Email>,
    phone: Option<String>,
    /// Purchase orders are emailed to the primary contact.
    primary: bool,
}

/// When the supplier is paid: `in_advance`, `on_receipt`, or `net` with its days, each
/// with an optional note ("2% off within 10 days").
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PaymentTermsDto {
    /// Before the goods are sent.
    InAdvance {
        #[serde(default)]
        note: Option<String>,
    },
    /// When the goods arrive.
    OnReceipt {
        #[serde(default)]
        note: Option<String>,
    },
    /// `days` (1 to 365) after the order.
    Net {
        days: u32,
        #[serde(default)]
        note: Option<String>,
    },
}

impl From<PaymentTermsDto> for PaymentTermsDraft {
    fn from(dto: PaymentTermsDto) -> Self {
        let (due, note) = match dto {
            PaymentTermsDto::InAdvance { note } => (PaymentDue::InAdvance, note),
            PaymentTermsDto::OnReceipt { note } => (PaymentDue::OnReceipt, note),
            PaymentTermsDto::Net { days, note } => (PaymentDue::Net { days }, note),
        };
        PaymentTermsDraft { due, note }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SupplierResponse {
    id: SupplierId,
    name: String,
    /// At most 10; at most one primary.
    contacts: Vec<SupplierContactResponse>,
    address: Option<PostalAddressResponse>,
    /// Its purchase orders' default currency, and its catalogue's.
    currency: Currency,
    payment_terms: Option<PaymentTermsDto>,
    /// The default for a purchase order's expected date, in days.
    lead_time_days: Option<u32>,
    tax_number: Option<String>,
    /// For staff only.
    notes: Option<String>,
    /// Archived suppliers take no new purchase orders.
    archived: bool,
    /// RFC 3339.
    created_at: String,
    updated_at: String,
}

impl SupplierResponse {
    fn new(s: &Supplier) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(s.stamps())?;
        Ok(Self {
            id: s.id(),
            name: s.name().to_string(),
            contacts: s
                .contacts()
                .iter()
                .map(|c| SupplierContactResponse {
                    name: c.name().to_string(),
                    email: c.email().cloned(),
                    phone: c.phone().map(str::to_string),
                    primary: c.primary(),
                })
                .collect(),
            address: s.address().map(PostalAddressResponse::from),
            currency: s.currency(),
            payment_terms: s.payment_terms().map(|t| {
                let note = t.note().map(str::to_string);
                match t.due() {
                    PaymentDue::InAdvance => PaymentTermsDto::InAdvance { note },
                    PaymentDue::OnReceipt => PaymentTermsDto::OnReceipt { note },
                    PaymentDue::Net { days } => PaymentTermsDto::Net { days, note },
                }
            }),
            lead_time_days: s.lead_time_days(),
            tax_number: s.tax_number().map(str::to_string),
            notes: s.notes().map(str::to_string),
            archived: s.archived(),
            created_at,
            updated_at,
        })
    }
}

/// Someone at a supplier, sent.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SupplierContactRequest {
    /// 1 to 255 characters.
    name: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    phone: Option<String>,
    /// At most one contact is; defaults to `false`.
    #[serde(default)]
    primary: bool,
}

impl From<SupplierContactRequest> for ContactDraft {
    fn from(c: SupplierContactRequest) -> Self {
        ContactDraft {
            name: c.name,
            email: c.email,
            phone: c.phone,
            primary: c.primary,
        }
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct SupplierFilters {
    /// Only archived ones (`true`) or only the others (`false`).
    archived: Option<bool>,
    /// In the name, ignoring case.
    q: Option<String>,
}

type SupplierPage = PageResponse<SupplierResponse>;

/// Suppliers, newest first.
#[utoipa::path(
    get,
    path = "/admin/suppliers",
    params(PageParams, SupplierFilters),
    responses((status = 200, description = "OK", body = SupplierPage)),
    extensions(("x-hypha-scopes" = json!(["purchasing:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<SupplierFilters>,
) -> Result<Json<SupplierPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse()?;
    let filter = SupplierFilter {
        archived: filters.archived,
        q: filters.q.filter(|q| !q.trim().is_empty()),
    };
    let page = list_suppliers(&ctx, &ports, filter, page).await?;
    Ok(Json(PageResponse::new(page, &scope, Supplier::id, |s| {
        SupplierResponse::new(&s)
    })?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSupplierRequest {
    /// 1 to 255 characters.
    name: String,
    /// At most 10; at most one primary.
    #[serde(default)]
    contacts: Vec<SupplierContactRequest>,
    #[serde(default)]
    address: Option<PostalAddressRequest>,
    /// ISO 4217: its purchase orders' default currency, and its catalogue's.
    currency: Currency,
    #[serde(default)]
    payment_terms: Option<PaymentTermsDto>,
    /// 0 to 365.
    #[serde(default)]
    lead_time_days: Option<u32>,
    #[serde(default)]
    tax_number: Option<String>,
    /// At most 5,000 characters.
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    archived: bool,
}

/// Adds a supplier.
#[utoipa::path(
    post,
    path = "/admin/suppliers",
    request_body = CreateSupplierRequest,
    responses((status = 201, description = "Created", body = SupplierResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateSupplierRequest>,
) -> Result<(StatusCode, Json<SupplierResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let draft = SupplierDraft {
        name: req.name,
        contacts: req.contacts.into_iter().map(Into::into).collect(),
        address: req.address.map(Into::into),
        currency: req.currency,
        payment_terms: req.payment_terms.map(Into::into),
        lead_time_days: req.lead_time_days,
        tax_number: req.tax_number,
        notes: req.notes,
        archived: req.archived,
    };
    let created = create_supplier(&ctx, &ports, &state.ids, draft).await?;
    Ok((StatusCode::CREATED, Json(SupplierResponse::new(&created)?)))
}

/// A supplier.
#[utoipa::path(
    get,
    path = "/admin/suppliers/{id}",
    params(("id" = SupplierId, Path)),
    responses((status = 200, description = "OK", body = SupplierResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<SupplierId>,
) -> Result<Json<SupplierResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let supplier = get_supplier(&ctx, &ports, id).await?;
    Ok(Json(SupplierResponse::new(&supplier)?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateSupplierRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    /// Replaces them all; `[]` for none.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<SupplierContactRequest>, required = false)]
    contacts: Option<Vec<SupplierContactRequest>>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<PostalAddressRequest>, required = false)]
    address: Option<Option<PostalAddressRequest>>,
    /// Refused (`409 supplier_currency_in_use`) while its catalogue has costs.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Currency, required = false)]
    currency: Option<Currency>,
    /// `null` clears them.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<PaymentTermsDto>, required = false)]
    payment_terms: Option<Option<PaymentTermsDto>>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<u32>, required = false)]
    lead_time_days: Option<Option<u32>>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    tax_number: Option<Option<String>>,
    /// `null` clears them.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    notes: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    archived: Option<bool>,
}

/// Changes a supplier (JSON Merge Patch), checked as a whole.
#[utoipa::path(
    patch,
    path = "/admin/suppliers/{id}",
    params(("id" = SupplierId, Path)),
    request_body = UpdateSupplierRequest,
    responses((status = 200, description = "OK", body = SupplierResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<SupplierId>,
    body: JsonBody<UpdateSupplierRequest>,
) -> Result<Json<SupplierResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateSupplier {
        name: req.name,
        contacts: req
            .contacts
            .map(|c| c.into_iter().map(Into::into).collect()),
        address: req.address.map(|a| a.map(Into::into)),
        currency: req.currency,
        payment_terms: req.payment_terms.map(|t| t.map(Into::into)),
        lead_time_days: req.lead_time_days,
        tax_number: req.tax_number,
        notes: req.notes,
        archived: req.archived,
    };
    let supplier = update_supplier(&ctx, &ports, id, input).await?;
    Ok(Json(SupplierResponse::new(&supplier)?))
}

/// Deletes a supplier with its catalogue. Refused while a purchase order names it
/// (`409 supplier_in_use`): archive it instead.
#[utoipa::path(
    delete,
    path = "/admin/suppliers/{id}",
    params(("id" = SupplierId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn remove<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<SupplierId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_supplier(&ctx, &ports, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- catalogues ----------

/// A variant a supplier sells, and on what terms.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SupplierVariantResponse {
    supplier: SupplierId,
    supplier_name: String,
    variant: VariantId,
    /// The product the variant belongs to, its title, the variant's option values ("M /
    /// Blue") and SKU: only with `products:read`, and `null` once the variant is gone.
    product: Option<ProductId>,
    product_title: Option<String>,
    variant_title: Option<String>,
    sku: Option<String>,
    /// The supplier's own code for it.
    supplier_sku: Option<String>,
    /// What the supplier charges per unit, in the supplier's currency.
    unit_cost: Option<UnitCost>,
    min_order_quantity: Option<u32>,
    /// Overrides the supplier's, in days.
    lead_time_days: Option<u32>,
}

/// The responses for `rows`, naming their variants' products when the caller may read
/// them.
async fn rows_response<P: HasProducts + HasSuppliers>(
    ports: &P,
    ctx: &Ctx,
    rows: Vec<SupplierVariant>,
) -> Result<Vec<SupplierVariantResponse>, ApiError> {
    let products: Vec<Product> = if ctx.has_scope(Scope::ProductsRead) {
        let variants: Vec<VariantId> = rows.iter().map(SupplierVariant::variant).collect();
        let owners = ports
            .products()
            .ids_of_variants(&variants)
            .await
            .map_err(hypha_products::CommandError::from)?;
        ports
            .products()
            .get_many(&owners)
            .await
            .map_err(hypha_products::CommandError::from)?
    } else {
        Vec::new()
    };
    let mut names: Vec<(SupplierId, String)> = Vec::new();
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let name = match names.iter().find(|(id, _)| *id == row.supplier()) {
            Some((_, name)) => name.clone(),
            None => {
                let name = ports
                    .suppliers()
                    .get(row.supplier())
                    .await
                    .map_err(hypha_purchasing::CommandError::from)?
                    .map(|s| s.name().to_string())
                    .unwrap_or_default();
                names.push((row.supplier(), name.clone()));
                name
            }
        };
        let product = products
            .iter()
            .find(|p| p.variants().iter().any(|v| v.id() == row.variant()));
        let variant = product.and_then(|p| p.variants().iter().find(|v| v.id() == row.variant()));
        out.push(SupplierVariantResponse {
            supplier: row.supplier(),
            supplier_name: name,
            variant: row.variant(),
            product: product.map(Product::id),
            product_title: product.map(|p| p.title().to_string()),
            variant_title: product.and_then(|p| p.variant_title(row.variant())),
            sku: variant.and_then(|v| v.sku()).map(str::to_string),
            supplier_sku: row.supplier_sku().map(str::to_string),
            unit_cost: row.unit_cost(),
            min_order_quantity: row.min_order_quantity(),
            lead_time_days: row.lead_time_days(),
        });
    }
    Ok(out)
}

type SupplierVariantPage = PageResponse<SupplierVariantResponse>;

/// What a supplier sells, newest variant first.
#[utoipa::path(
    get,
    path = "/admin/suppliers/{id}/variants",
    params(("id" = SupplierId, Path), PageParams),
    responses((status = 200, description = "OK", body = SupplierVariantPage)),
    extensions(("x-hypha-scopes" = json!(["purchasing:read"]))),
)]
pub async fn catalogue<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<SupplierId>,
    query: ListQuery,
) -> Result<Json<SupplierVariantPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, _, scope) = query.parse()?;
    let page = list_supplier_variants(&ctx, &ports, id, page).await?;
    let has_more = page.has_more;
    let ids: Vec<VariantId> = page.items.iter().map(SupplierVariant::variant).collect();
    let rows = rows_response(&ports, &ctx, page.items).await?;
    let page = hypha_core::Page {
        items: ids.into_iter().zip(rows).collect::<Vec<_>>(),
        has_more,
    };
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |(id, _)| *id,
        |(_, row)| Ok(row),
    )?))
}

/// A catalogue row, sent: replaces the one the supplier had for the variant.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSupplierVariantRequest {
    /// The supplier's own code for it, printed on purchase orders.
    #[serde(default)]
    supplier_sku: Option<String>,
    /// Per unit, in the supplier's currency (`422 cost_currency_mismatch` otherwise).
    #[serde(default)]
    unit_cost: Option<UnitCost>,
    /// At least 1.
    #[serde(default)]
    min_order_quantity: Option<u32>,
    /// 0 to 365; overrides the supplier's.
    #[serde(default)]
    lead_time_days: Option<u32>,
}

/// Sets what a supplier charges for a variant, replacing what it had.
#[utoipa::path(
    put,
    path = "/admin/suppliers/{id}/variants/{variant_id}",
    params(("id" = SupplierId, Path), ("variant_id" = VariantId, Path)),
    request_body = SetSupplierVariantRequest,
    responses((status = 200, description = "OK", body = SupplierVariantResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn set_variant<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, variant)): PathId<(SupplierId, VariantId)>,
    body: JsonBody<SetSupplierVariantRequest>,
) -> Result<Json<SupplierVariantResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let exists = ports
        .products()
        .get_by_variant(variant)
        .await
        .map_err(hypha_products::CommandError::from)?
        .is_some();
    if !exists {
        return Err(
            hypha_products::CommandError::Product(ProductError::VariantNotFound(variant)).into(),
        );
    }
    let draft = SupplierVariantDraft {
        supplier_sku: req.supplier_sku,
        unit_cost: req.unit_cost,
        min_order_quantity: req.min_order_quantity,
        lead_time_days: req.lead_time_days,
    };
    let row = set_supplier_variant(&ctx, &ports, id, variant, draft).await?;
    let mut rows = rows_response(&ports, &ctx, vec![row]).await?;
    rows.pop()
        .map(Json)
        .ok_or_else(|| ApiError::internal("a catalogue row without a response"))
}

/// Takes a variant off a supplier's catalogue.
#[utoipa::path(
    delete,
    path = "/admin/suppliers/{id}/variants/{variant_id}",
    params(("id" = SupplierId, Path), ("variant_id" = VariantId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["purchasing:write"]))),
)]
pub async fn remove_variant<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, variant)): PathId<(SupplierId, VariantId)>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    remove_supplier_variant(&ctx, &ports, id, variant).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct VariantSupplierListResponse {
    /// By supplier ID.
    items: Vec<SupplierVariantResponse>,
}

/// Who sells a variant, and at what.
#[utoipa::path(
    get,
    path = "/admin/variants/{id}/suppliers",
    params(("id" = VariantId, Path)),
    responses((status = 200, description = "OK", body = VariantSupplierListResponse)),
    extensions(("x-hypha-scopes" = json!(["purchasing:read"]))),
)]
pub async fn of_variant<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(variant): PathId<VariantId>,
) -> Result<Json<VariantSupplierListResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let rows = variant_suppliers(&ctx, &ports, variant).await?;
    Ok(Json(VariantSupplierListResponse {
        items: rows_response(&ports, &ctx, rows).await?,
    }))
}
