//! Custom fields (design note 0033): definitions in the merchant's `custom` namespace,
//! an owner's values (products, variants, categories, customers, orders), the bulk list
//! and value writes, and what the other routes share: who the caller is to the fields
//! (`field_caller`), their values on owner responses (`read`) and the `field[…]` list
//! filters (`list_fields`).
//!
//! Owners are checked here, through their own commands, before the leaf sees them
//! (§6.1): one that doesn't exist is the owner's own 404 in a URL, and
//! `422 owner_not_found` in a bulk body.

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::{Json, extract::State, http::StatusCode};
use hypha_apps::{AppRepo, HasApps};
use hypha_core::{
    Actor, CategoryId, Ctx, CustomFieldDefinitionId, CustomerId, FieldFilter, OrderId, PageLimit,
    ProductId, Stamped, VariantId,
};
use hypha_custom_fields::{
    Access, AdminAccess, CallerKind, Capabilities, Choice, CustomFieldDefinition, DefinitionChange,
    DefinitionRepo, DefinitionSpec, FieldCaller, FieldKey, FieldType, FieldsRead,
    HasCustomFieldDefinitions, HasCustomFieldValues, Owner, OwnerType, RawValidations, Reference,
    ReferenceKind, ReferenceTargets, SetCustomFields, StorefrontAccess, change_list_items,
    create_definition, definitions_for, delete_definition, field_filter, get_definition,
    list_definitions, move_definition, read_custom_fields, set_custom_fields, set_field_values,
    update_definition, values_in_use,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::openapi::{
    ObjectBuilder, Required, Type,
    path::{Parameter, ParameterBuilder, ParameterIn, ParameterStyle},
    schema::AdditionalProperties,
};

use super::{
    page::{FieldQuery, RawFieldFilter},
    stamp_times,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId, Query},
    patch,
    state::{SharedState, Storage},
};

// ---------- who is calling, and what it sees ----------

/// What the caller is to custom fields (§5.1): an app by its key (its OAuth token names
/// its ID), a spore's own app (`spore_<name>`) as the spore, anyone else on the admin
/// surface as the merchant.
pub(crate) async fn caller_kind<P: HasApps>(ctx: &Ctx, ports: &P) -> Result<CallerKind, ApiError> {
    let Actor::App(id) = ctx.actor() else {
        return Ok(CallerKind::Merchant);
    };
    // An app that's gone since its token was checked owns nothing: the leaf treats an
    // unnamed app as neither the merchant nor an owner.
    let Some(app) = ports.apps().get_by_id(id).await? else {
        return Ok(CallerKind::Merchant);
    };
    let key = app.key().clone();
    Ok(match key.as_str().starts_with("spore_") {
        true => CallerKind::Spore(key),
        false => CallerKind::App(key),
    })
}

/// An owner's values as JSON, `{"custom.material": "wool", …}`, as every response
/// carries them.
pub type CustomFieldValues = Map<String, Value>;

/// Every value of `owners` (each one of `owner_types`) the caller may read, or only
/// `keys` (a list's `?custom_fields=`), with the personal keys hidden from it.
pub(crate) async fn read<P>(
    ctx: &Ctx,
    kind: &CallerKind,
    ports: &P,
    owner_types: &[OwnerType],
    owners: &[uuid::Uuid],
    keys: Option<&[FieldKey]>,
) -> Result<FieldsRead, ApiError>
where
    P: HasCustomFieldDefinitions + HasCustomFieldValues,
{
    let caller = FieldCaller::new(ctx, kind);
    Ok(read_custom_fields(&caller, ports, owner_types, owners, keys).await?)
}

/// What a list asked of its owners' custom fields (§6.2, §7): the filters, checked
/// against `owner_type`'s definitions, and the keys to include (`None`: none).
pub(crate) struct ListFields {
    pub filters: Vec<FieldFilter>,
    pub keys: Option<Vec<FieldKey>>,
    pub kind: CallerKind,
}

/// Checks a list's `field[…]` filters and `custom_fields` keys (`FieldQuery`): an
/// unknown or hidden key is `422 custom_field_unknown`, one that isn't filterable `422
/// custom_field_not_filterable`, a personal one without `customer_data:profile` a 403;
/// never ignored (CF7). Reads nothing when the list asked for neither.
pub(crate) async fn list_fields<P>(
    ctx: &Ctx,
    ports: &P,
    owner_type: OwnerType,
    query: &FieldQuery,
) -> Result<ListFields, ApiError>
where
    P: HasApps + HasCustomFieldDefinitions,
{
    if query.is_empty() {
        return Ok(ListFields {
            filters: Vec::new(),
            keys: None,
            kind: CallerKind::Merchant,
        });
    }
    let kind = caller_kind(ctx, ports).await?;
    let caller = FieldCaller::new(ctx, &kind);
    let mut filters = Vec::with_capacity(query.filters.len());
    if !query.filters.is_empty() {
        let definitions = definitions_for(ports, owner_type).await?;
        for RawFieldFilter { key, op, operands } in &query.filters {
            filters.push(field_filter(&caller, &definitions, key, *op, operands)?);
        }
    }
    let keys = query
        .keys
        .as_ref()
        .map(|keys| {
            keys.iter()
                .map(|k| {
                    FieldKey::parse(k)
                        .map_err(|_| hypha_custom_fields::CommandError::Unknown(k.clone()))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    Ok(ListFields {
        filters,
        keys,
        kind,
    })
}

/// `field[…]` and `custom_fields` in the documents: the keys are data, so the pattern
/// is documented once per list route (§7).
pub struct FieldParams;

impl utoipa::IntoParams for FieldParams {
    fn into_params(_: impl Fn() -> Option<ParameterIn>) -> Vec<Parameter> {
        let ops = ObjectBuilder::new()
            .property("eq", ObjectBuilder::new().schema_type(Type::String))
            .property("gt", ObjectBuilder::new().schema_type(Type::String))
            .property("gte", ObjectBuilder::new().schema_type(Type::String))
            .property("lt", ObjectBuilder::new().schema_type(Type::String))
            .property("lte", ObjectBuilder::new().schema_type(Type::String))
            .property("exists", ObjectBuilder::new().schema_type(Type::String));
        let filter = utoipa::openapi::schema::OneOfBuilder::new()
            .item(ObjectBuilder::new().schema_type(Type::String))
            .item(ops);
        vec![
            ParameterBuilder::new()
                .name("field")
                .parameter_in(ParameterIn::Query)
                .required(Required::False)
                .style(Some(ParameterStyle::DeepObject))
                .explode(Some(true))
                .description(Some(
                    "Filters on custom fields (design note 0033, §7), only `filterable` ones: \
                     `field[custom.material]=wool,cotton` (any of these; a list field holds \
                     any of them), `field[custom.weight][gte]=1.5` (`gt`, `gte`, `lt`, `lte`: \
                     numbers, money in minor units, measurements like `1.5kg`, dates and \
                     times), `field[custom.erp_id][exists]=false`, \
                     `field[custom.flag]=true`. Every filter must match. Text compares \
                     ignoring case; a `unique` field's filter finds at most one. An unknown \
                     key is `422 custom_field_unknown`, one that isn't filterable `422 \
                     custom_field_not_filterable`, a personal one needs \
                     `customer_data:profile`.",
                ))
                .schema(Some(
                    ObjectBuilder::new()
                        .schema_type(Type::Object)
                        .additional_properties(Some(AdditionalProperties::RefOr(filter.into()))),
                ))
                .build(),
            ParameterBuilder::new()
                .name("custom_fields")
                .parameter_in(ParameterIn::Query)
                .required(Required::False)
                .description(Some(
                    "Comma-separated `namespace.key`s (at most 10) whose values each item \
                     carries in `custom_fields`; without it, items carry none.",
                ))
                .schema(Some(ObjectBuilder::new().schema_type(Type::String)))
                .build(),
        ]
    }
}

/// `custom_fields.<key>` for each hidden personal key, as a response's `redacted` names
/// them (design note 0033, §5.2).
pub(crate) fn redacted_names(keys: &[FieldKey]) -> Vec<String> {
    keys.iter().map(|k| format!("custom_fields.{k}")).collect()
}

// ---------- definitions ----------

/// The kinds of record fields belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OwnerTypeDto {
    Product,
    Variant,
    /// A product category.
    Category,
    Customer,
    Order,
}

impl From<OwnerTypeDto> for OwnerType {
    fn from(t: OwnerTypeDto) -> Self {
        match t {
            OwnerTypeDto::Product => OwnerType::Product,
            OwnerTypeDto::Variant => OwnerType::Variant,
            OwnerTypeDto::Category => OwnerType::Category,
            OwnerTypeDto::Customer => OwnerType::Customer,
            OwnerTypeDto::Order => OwnerType::Order,
        }
    }
}

impl From<OwnerType> for OwnerTypeDto {
    fn from(t: OwnerType) -> Self {
        match t {
            OwnerType::Product => OwnerTypeDto::Product,
            OwnerType::Variant => OwnerTypeDto::Variant,
            OwnerType::Category => OwnerTypeDto::Category,
            OwnerType::Customer => OwnerTypeDto::Customer,
            OwnerType::Order => OwnerTypeDto::Order,
        }
    }
}

/// A `choice` field's choice: the key is what's stored, filtered and faceted (it can't
/// change); the label is for people.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = CustomFieldChoice)]
#[serde(deny_unknown_fields)]
pub struct ChoiceDto {
    /// `[a-z0-9_-]{1,64}`.
    key: String,
    label: String,
}

/// A definition's validations; which apply depends on its type (design note 0033, §3).
/// Absent ones don't apply.
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = CustomFieldValidations)]
#[serde(deny_unknown_fields)]
pub struct ValidationsDto {
    /// Text, in characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_length: Option<u32>,
    /// `text`: a regular expression the whole value matches (no look-around or
    /// back-references).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pattern: Option<String>,
    /// In the type's own terms: an integer, a decimal string, a date or date-time,
    /// money's minor units, a measurement `{"value", "unit"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<serde_json::Value>)]
    min: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<serde_json::Value>)]
    max: Option<Value>,
    /// `decimal`: digits after the point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_scale: Option<u32>,
    /// `url`: `https`, `http`, `mailto`, `tel`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schemes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    domains: Option<Vec<String>>,
    /// `money`: ISO 4217 codes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    currencies: Option<Vec<String>>,
    /// `measurement` (required): `weight`, `length` or `volume`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dimension: Option<String>,
    /// `choice` (required): at most 128.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    choices: Option<Vec<ChoiceDto>>,
    /// `reference` (required): `product`, `variant`, `category`, `customer` or `asset`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    /// Lists: at most 128 items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_items: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_items: Option<u32>,
    /// Lists: no item twice (text ignoring case).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unique_items: Option<bool>,
}

impl From<ValidationsDto> for RawValidations {
    fn from(v: ValidationsDto) -> Self {
        RawValidations {
            min_length: v.min_length,
            max_length: v.max_length,
            pattern: v.pattern,
            min: v.min,
            max: v.max,
            max_scale: v.max_scale,
            schemes: v.schemes,
            domains: v.domains,
            currencies: v.currencies,
            dimension: v.dimension,
            choices: v.choices.map(|choices| {
                choices
                    .into_iter()
                    .map(|c| Choice {
                        key: c.key,
                        label: c.label,
                    })
                    .collect()
            }),
            kind: v.kind,
            min_items: v.min_items,
            max_items: v.max_items,
            unique_items: v.unique_items,
        }
    }
}

impl From<RawValidations> for ValidationsDto {
    fn from(v: RawValidations) -> Self {
        Self {
            min_length: v.min_length,
            max_length: v.max_length,
            pattern: v.pattern,
            min: v.min,
            max: v.max,
            max_scale: v.max_scale,
            schemes: v.schemes,
            domains: v.domains,
            currencies: v.currencies,
            dimension: v.dimension,
            choices: v.choices.map(|choices| {
                choices
                    .into_iter()
                    .map(|c| ChoiceDto {
                        key: c.key,
                        label: c.label,
                    })
                    .collect()
            }),
            kind: v.kind,
            min_items: v.min_items,
            max_items: v.max_items,
            unique_items: v.unique_items,
        }
    }
}

/// What staff may do with an app's or a spore's field; the merchant's own are always
/// `read_write`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = CustomFieldAdminAccess)]
#[serde(rename_all = "snake_case")]
pub enum AdminAccessDto {
    Read,
    ReadWrite,
}

/// Whether storefronts see the field (opt-in).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = CustomFieldStorefrontAccess)]
#[serde(rename_all = "snake_case")]
pub enum StorefrontAccessDto {
    None,
    Read,
}

/// Who sees and writes a field's values (design note 0033, §5).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = CustomFieldAccess)]
#[serde(deny_unknown_fields)]
pub struct AccessDto {
    admin: AdminAccessDto,
    storefront: StorefrontAccessDto,
}

impl From<AccessDto> for Access {
    fn from(a: AccessDto) -> Self {
        Access {
            admin: match a.admin {
                AdminAccessDto::Read => AdminAccess::Read,
                AdminAccessDto::ReadWrite => AdminAccess::ReadWrite,
            },
            storefront: match a.storefront {
                StorefrontAccessDto::None => StorefrontAccess::None,
                StorefrontAccessDto::Read => StorefrontAccess::Read,
            },
        }
    }
}

impl From<Access> for AccessDto {
    fn from(a: Access) -> Self {
        Self {
            admin: match a.admin {
                AdminAccess::Read => AdminAccessDto::Read,
                AdminAccess::ReadWrite => AdminAccessDto::ReadWrite,
            },
            storefront: match a.storefront {
                StorefrontAccess::None => StorefrontAccessDto::None,
                StorefrontAccess::Read => StorefrontAccessDto::Read,
            },
        }
    }
}

/// What a field's values can be used for (§7).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = CustomFieldCapabilities)]
#[serde(deny_unknown_fields)]
pub struct CapabilitiesDto {
    /// A filter on its owner's admin list (`field[custom.material]=wool`); at most 50
    /// per owner type.
    #[serde(default)]
    filterable: bool,
    /// No two owners hold the same value (text ignoring case): `text`, `integer` and
    /// `url` only. Filtering on it finds at most one owner.
    #[serde(default)]
    unique: bool,
}

impl From<CapabilitiesDto> for Capabilities {
    fn from(c: CapabilitiesDto) -> Self {
        Capabilities {
            filterable: c.filterable,
            unique: c.unique,
        }
    }
}

impl From<Capabilities> for CapabilitiesDto {
    fn from(c: Capabilities) -> Self {
        Self {
            filterable: c.filterable,
            unique: c.unique,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CustomFieldDefinitionResponse {
    id: CustomFieldDefinitionId,
    owner_type: OwnerTypeDto,
    /// `custom` (the merchant's), `app_<name>` or `spore_<name>`.
    namespace: String,
    key: String,
    /// `namespace.key`: what values, filters and events name it by.
    field: String,
    name: String,
    description: Option<String>,
    /// `text`, `integer`, `choice`, `list.text`, … (design note 0033, §3).
    #[serde(rename = "type")]
    field_type: String,
    validations: ValidationsDto,
    access: AccessDto,
    /// Only its app (and staff) read it.
    private: bool,
    /// About a person: redacted without `customer_data:profile` (customers and orders).
    personal: bool,
    capabilities: CapabilitiesDto,
    /// On its owner type's form, from 0.
    position: u32,
    /// The app or spore that declares it (`app_<name>`, `spore_<name>`); `null` for the
    /// merchant's own. Those are read-only here (`409 custom_field_managed`).
    managed_by: Option<String>,
    /// RFC 3339 in UTC.
    created_at: String,
    updated_at: String,
}

impl CustomFieldDefinitionResponse {
    fn new(d: &CustomFieldDefinition) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(d.stamps())?;
        Ok(Self {
            id: d.id(),
            owner_type: d.owner_type().into(),
            namespace: d.namespace().to_string(),
            key: d.key().to_string(),
            field: d.field_key().to_string(),
            name: d.name().to_string(),
            description: d.description().map(str::to_string),
            field_type: d.field_type().to_string(),
            validations: d.validations().to_raw().into(),
            access: d.access().into(),
            private: d.private(),
            personal: d.personal(),
            capabilities: d.capabilities().into(),
            position: d.position(),
            managed_by: d.namespace().managed_by().map(|k| k.to_string()),
            created_at,
            updated_at,
        })
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CustomFieldDefinitionList {
    /// By owner type, then position. A store has at most 128 per owner type and
    /// namespace, so they come in one response.
    items: Vec<CustomFieldDefinitionResponse>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct DefinitionFilters {
    /// Only this owner type's.
    #[param(inline)]
    owner_type: Option<OwnerTypeDto>,
}

/// Every namespace's field definitions (the merchant's, apps' and spores').
#[utoipa::path(
    get,
    path = "/admin/custom-field-definitions",
    params(DefinitionFilters),
    responses((status = 200, description = "OK", body = CustomFieldDefinitionList)),
    extensions(("x-hypha-scopes" = json!(["custom_fields:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: Query<DefinitionFilters>,
) -> Result<Json<CustomFieldDefinitionList>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let filters = query.parse()?;
    let definitions = list_definitions(&ctx, &ports, filters.owner_type.map(Into::into)).await?;
    Ok(Json(CustomFieldDefinitionList {
        items: definitions
            .iter()
            .map(CustomFieldDefinitionResponse::new)
            .collect::<Result<_, _>>()?,
    }))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCustomFieldDefinitionRequest {
    owner_type: OwnerTypeDto,
    /// `[a-z0-9_]{2,64}`, starting with a letter; unique per owner type in `custom`.
    key: String,
    /// `text`, `multiline_text`, `rich_text`, `integer`, `decimal`, `boolean`, `date`,
    /// `date_time`, `url`, `money`, `measurement`, `choice`, `reference`, `json`, or
    /// `list.<type>` (not of `boolean`, `multiline_text`, `rich_text`, `json`). It never
    /// changes.
    #[serde(rename = "type")]
    field_type: String,
    /// Up to 255 characters.
    name: String,
    /// Up to 1,000 characters.
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    validations: Option<ValidationsDto>,
    /// Default: admin `read_write`, storefront `none`.
    #[serde(default)]
    access: Option<AccessDto>,
    /// Customers' and orders' fields only.
    #[serde(default)]
    personal: bool,
    #[serde(default)]
    capabilities: Option<CapabilitiesDto>,
}

/// Creates a field in the merchant's `custom` namespace, last on its owner type's form.
#[utoipa::path(
    post,
    path = "/admin/custom-field-definitions",
    request_body = CreateCustomFieldDefinitionRequest,
    responses((status = 201, description = "Created", body = CustomFieldDefinitionResponse)),
    extensions(("x-hypha-scopes" = json!(["custom_fields:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateCustomFieldDefinitionRequest>,
) -> Result<(StatusCode, Json<CustomFieldDefinitionResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let field_type = FieldType::parse(&req.field_type).map_err(|e| {
        hypha_custom_fields::CommandError::from(hypha_custom_fields::DefinitionError::from(e))
    })?;
    let spec = DefinitionSpec {
        owner_type: req.owner_type.into(),
        key: req.key,
        field_type,
        name: req.name,
        description: req.description,
        validations: req.validations.unwrap_or_default().into(),
        access: req.access.map(Into::into).unwrap_or_default(),
        private: false,
        personal: req.personal,
        capabilities: req.capabilities.map(Into::into).unwrap_or_default(),
    };
    let created = create_definition(&ctx, &ports, &state.ids, spec).await?;
    Ok((
        StatusCode::CREATED,
        Json(CustomFieldDefinitionResponse::new(&created)?),
    ))
}

/// A field definition.
#[utoipa::path(
    get,
    path = "/admin/custom-field-definitions/{id}",
    params(("id" = CustomFieldDefinitionId, Path)),
    responses((status = 200, description = "OK", body = CustomFieldDefinitionResponse)),
    extensions(("x-hypha-scopes" = json!(["custom_fields:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomFieldDefinitionId>,
) -> Result<Json<CustomFieldDefinitionResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let definition = get_definition(&ctx, &ports, id).await?;
    Ok(Json(CustomFieldDefinitionResponse::new(&definition)?))
}

/// A JSON Merge Patch of a `custom` definition: absent keeps; `null` clears the
/// description. Owner type, key and type never change.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCustomFieldDefinitionRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    description: Option<Option<String>>,
    /// Replaces them all. A change that tightens them (a lower `max`, fewer `choices`, a
    /// `pattern`) is checked against the stored values first: `409
    /// custom_field_values_invalid` names the owners to fix.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = ValidationsDto, required = false)]
    validations: Option<ValidationsDto>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = AccessDto, required = false)]
    access: Option<AccessDto>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    personal: Option<bool>,
    /// Turning `unique` on is checked against the stored values, like validations.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = CapabilitiesDto, required = false)]
    capabilities: Option<CapabilitiesDto>,
}

/// Changes a field of the merchant's. An app's or a spore's is `409
/// custom_field_managed`: its manifest or code declares it.
#[utoipa::path(
    patch,
    path = "/admin/custom-field-definitions/{id}",
    params(("id" = CustomFieldDefinitionId, Path)),
    request_body = UpdateCustomFieldDefinitionRequest,
    responses((status = 200, description = "OK", body = CustomFieldDefinitionResponse)),
    extensions(("x-hypha-scopes" = json!(["custom_fields:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomFieldDefinitionId>,
    body: JsonBody<UpdateCustomFieldDefinitionRequest>,
) -> Result<Json<CustomFieldDefinitionResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let change = DefinitionChange {
        name: req.name,
        description: req.description,
        validations: req.validations.map(Into::into),
        access: req.access.map(Into::into),
        private: None,
        personal: req.personal,
        capabilities: req.capabilities.map(Into::into),
    };
    let definition = update_definition(&ctx, &ports, id, change).await?;
    Ok(Json(CustomFieldDefinitionResponse::new(&definition)?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveCustomFieldDefinitionRequest {
    /// From 0 on its owner type's form; past the end: last.
    position: u32,
}

/// Moves a field of the merchant's on its owner type's form; the others close up.
#[utoipa::path(
    put,
    path = "/admin/custom-field-definitions/{id}/position",
    params(("id" = CustomFieldDefinitionId, Path)),
    request_body = MoveCustomFieldDefinitionRequest,
    responses((status = 200, description = "OK", body = CustomFieldDefinitionResponse)),
    extensions(("x-hypha-scopes" = json!(["custom_fields:write"]))),
)]
pub async fn move_to<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomFieldDefinitionId>,
    body: JsonBody<MoveCustomFieldDefinitionRequest>,
) -> Result<Json<CustomFieldDefinitionResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let definition = move_definition(&ctx, &ports, id, req.position).await?;
    Ok(Json(CustomFieldDefinitionResponse::new(&definition)?))
}

/// Deletes a field of the merchant's with every value of it.
#[utoipa::path(
    delete,
    path = "/admin/custom-field-definitions/{id}",
    params(("id" = CustomFieldDefinitionId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["custom_fields:write"]))),
)]
pub async fn remove<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomFieldDefinitionId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_definition(&ctx, &ports, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ValueInUseResponse {
    /// A value (a list's item, for a list field) as stored: a choice's key, text as
    /// first written.
    #[schema(value_type = serde_json::Value)]
    value: Value,
    /// How many owners hold it.
    owners: u64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ValuesInUseResponse {
    /// Most held first.
    items: Vec<ValueInUseResponse>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ValuesInUseParams {
    /// How many values, 1 to 100 (default 50).
    limit: Option<u32>,
}

/// The distinct values of a filterable field with how many owners hold each: for
/// autocomplete and filter menus. Needs the read scope of the field's owner type
/// (`products:read`, `customers:read`, `orders:read`), and `customer_data:profile` for a
/// personal one.
#[utoipa::path(
    get,
    path = "/admin/custom-field-definitions/{id}/values-in-use",
    params(("id" = CustomFieldDefinitionId, Path), ValuesInUseParams),
    responses((status = 200, description = "OK", body = ValuesInUseResponse)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn values<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomFieldDefinitionId>,
    query: Query<ValuesInUseParams>,
) -> Result<Json<ValuesInUseResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let params = query.parse()?;
    let limit = match params.limit {
        Some(limit) => PageLimit::new(limit)?,
        None => PageLimit::DEFAULT,
    };
    let kind = caller_kind(&ctx, &ports).await?;
    let caller = FieldCaller::new(&ctx, &kind);
    let found = values_in_use(&caller, &ports, id, limit).await?;
    Ok(Json(ValuesInUseResponse {
        items: found
            .into_iter()
            .map(|v| ValueInUseResponse {
                value: v.value,
                owners: v.owners,
            })
            .collect(),
    }))
}

// ---------- an owner's values ----------

/// An owner's values as the caller sees them, with versions for `if_versions`.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CustomFieldsResponse {
    /// The owner (`prod_…`, `var_…`, `pcat_…`, `cus_…`, `ord_…`).
    owner: String,
    /// `{"custom.material": "wool", …}`: every value the caller may read, by the
    /// definitions' order.
    #[schema(value_type = std::collections::HashMap<String, serde_json::Value>)]
    custom_fields: CustomFieldValues,
    /// Each value's version (`{"custom.material": 3}`), for `if_versions`.
    versions: BTreeMap<String, u64>,
    /// Personal fields left out for this caller (`custom_fields.<key>`): an API key or
    /// app without `customer_data:profile`. Never sent to staff.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
}

impl CustomFieldsResponse {
    fn new(owner: &Owner, read: &FieldsRead) -> Self {
        let fields = read.owners.get(&owner.id());
        let redacted = redacted_names(&read.redacted);
        Self {
            owner: owner.prefixed(),
            custom_fields: read.json_of(owner.id()),
            versions: fields.map(|f| f.versions()).unwrap_or_default(),
            redacted: (!redacted.is_empty()).then_some(redacted),
        }
    }
}

/// `owner`'s values, after the route checked it exists.
async fn respond_owner<S: Storage>(
    ports: &S::Ports<'_>,
    ctx: &Ctx,
    kind: &CallerKind,
    owner: &Owner,
) -> Result<Json<CustomFieldsResponse>, ApiError> {
    let read = read(ctx, kind, ports, &[owner.kind()], &[owner.id()], None).await?;
    Ok(Json(CustomFieldsResponse::new(owner, &read)))
}

/// A write to an owner's fields: a JSON Merge Patch of its values.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCustomFieldsRequest {
    /// `{"custom.material": "wool", "custom.care": null}`: a key absent keeps its value,
    /// `null` removes it. At most 50 keys; every problem is reported at once (`422
    /// custom_fields_invalid`), and nothing is written unless all are valid.
    #[schema(value_type = std::collections::HashMap<String, serde_json::Value>)]
    custom_fields: Map<String, Value>,
    /// Writes a key only if its value's version is still this one (`0`: no value yet);
    /// otherwise `409 custom_field_stale`. Optional: by default the last write wins.
    #[serde(default)]
    if_versions: BTreeMap<String, u64>,
}

impl From<SetCustomFieldsRequest> for SetCustomFields {
    fn from(r: SetCustomFieldsRequest) -> Self {
        SetCustomFields {
            values: r.custom_fields.into_iter().collect(),
            if_versions: r.if_versions,
        }
    }
}

/// Writes `owner`'s values, after the route checked it exists.
async fn write_owner<S: Storage>(
    ports: &S::Ports<'_>,
    ctx: &Ctx,
    owner: &Owner,
    req: SetCustomFieldsRequest,
) -> Result<Json<CustomFieldsResponse>, ApiError> {
    let kind = caller_kind(ctx, ports).await?;
    let caller = FieldCaller::new(ctx, &kind);
    set_custom_fields(&caller, ports, owner, req.into()).await?;
    respond_owner::<S>(ports, ctx, &kind, owner).await
}

/// Generates the `GET`/`PATCH …/{id}/custom-fields` pair of an owner type: `$load`
/// checks the owner exists (its own 404) and gives its `Owner`.
macro_rules! owner_routes {
    ($get:ident, $patch:ident, $path:literal, $id:ty, $scope_read:literal, $scope_write:literal,
     |$ctx:ident, $ports:ident, $owner_id:ident| $load:expr) => {
        /// The owner's custom field values the caller may read, with their versions.
        #[utoipa::path(
                    get,
                    path = $path,
                    params(("id" = $id, Path)),
                    responses((status = 200, description = "OK", body = CustomFieldsResponse)),
                    extensions(("x-hypha-scopes" = json!([$scope_read]))),
                )]
        pub async fn $get<S: Storage>(
            State(state): State<SharedState<S>>,
            caller: Caller,
            PathId($owner_id): PathId<$id>,
        ) -> Result<Json<CustomFieldsResponse>, ApiError> {
            let ($ports, $ctx) = caller.authenticate(&state).await?;
            let ($ports, $ctx) = (&$ports, &$ctx);
            let owner: Owner = $load;
            let kind = caller_kind($ctx, $ports).await?;
            respond_owner::<S>($ports, $ctx, &kind, &owner).await
        }

        /// Sets and removes the owner's custom field values (a JSON Merge Patch), all
        /// or nothing, recording one `<owner>.custom_fields_updated` event. Each key
        /// needs the caller to be allowed to write it: anyone with the owner's write
        /// scope for `custom.*`, only its app for `app_<name>.*` (staff too when the
        /// field's `access.admin` is `read_write`), `customer_data:profile` for a
        /// personal field.
        #[utoipa::path(
                    patch,
                    path = $path,
                    params(("id" = $id, Path)),
                    request_body = SetCustomFieldsRequest,
                    responses((status = 200, description = "OK", body = CustomFieldsResponse)),
                    extensions(("x-hypha-scopes" = json!([$scope_write]))),
                )]
        pub async fn $patch<S: Storage>(
            State(state): State<SharedState<S>>,
            caller: Caller,
            PathId($owner_id): PathId<$id>,
            body: JsonBody<SetCustomFieldsRequest>,
        ) -> Result<Json<CustomFieldsResponse>, ApiError> {
            let ($ports, $ctx) = caller.authenticate(&state).await?;
            let req = body.parse()?;
            let ($ports, $ctx) = (&$ports, &$ctx);
            let owner: Owner = $load;
            write_owner::<S>($ports, $ctx, &owner, req).await
        }
    };
}

owner_routes!(
    fetch_product,
    update_product,
    "/admin/products/{id}/custom-fields",
    ProductId,
    "products:read",
    "products:write",
    |ctx, ports, id| {
        hypha_products::get_product(ctx, hypha_products::HasProducts::products(ports), id).await?;
        Owner::product(id)
    }
);

owner_routes!(
    fetch_variant,
    update_variant,
    "/admin/variants/{id}/custom-fields",
    VariantId,
    "products:read",
    "products:write",
    |ctx, ports, id| {
        let product = hypha_products::get_variant_product(
            ctx,
            hypha_products::HasProducts::products(ports),
            id,
        )
        .await?;
        Owner::variant(product.id(), id)
    }
);

owner_routes!(
    fetch_category,
    update_category,
    "/admin/categories/{id}/custom-fields",
    CategoryId,
    "products:read",
    "products:write",
    |ctx, ports, id| {
        hypha_categories::get_category(ctx, ports, id)
            .await
            .map_err(|e| ApiError::from(e).not_found_if("category_not_found"))?;
        Owner::category(id)
    }
);

owner_routes!(
    fetch_customer,
    update_customer,
    "/admin/customers/{id}/custom-fields",
    CustomerId,
    "customers:read",
    "customers:write",
    |ctx, ports, id| {
        hypha_customers::get_customer(ctx, hypha_customers::HasCustomers::customers(ports), id)
            .await?;
        Owner::customer(id)
    }
);

owner_routes!(
    fetch_order,
    update_order,
    "/admin/orders/{id}/custom-fields",
    OrderId,
    "orders:read",
    "orders:write",
    |ctx, ports, id| {
        hypha_orders::get_order(ctx, hypha_orders::HasOrders::orders(ports), id).await?;
        Owner::order(id)
    }
);

// ---------- bulk ----------

/// The owners `ids` name, as the definition's owner type, each checked to exist (§6.1):
/// a string that isn't that type's ID is `422 invalid_owner`, one that names nothing
/// `422 owner_not_found`, each listing the owners. Needs the owner type's write scope
/// first, so a caller without it learns nothing about which exist.
async fn bulk_owners<S: Storage>(
    ports: &S::Ports<'_>,
    ctx: &Ctx,
    definition: CustomFieldDefinitionId,
    ids: &[String],
) -> Result<Vec<Owner>, ApiError> {
    let definition = ports
        .custom_field_definitions()
        .get(definition)
        .await?
        .ok_or(hypha_custom_fields::CommandError::DefinitionNotFound(
            definition,
        ))?;
    let owner_type = definition.owner_type();
    ctx.require(owner_type.write_scope())
        .map_err(hypha_custom_fields::CommandError::from)?;
    fn parse_all<T: std::str::FromStr>(ids: &[String]) -> Result<Vec<T>, Vec<String>> {
        let mut bad = Vec::new();
        let mut good = Vec::with_capacity(ids.len());
        for id in ids {
            match id.parse() {
                Ok(parsed) => good.push(parsed),
                Err(_) => bad.push(id.clone()),
            }
        }
        if bad.is_empty() { Ok(good) } else { Err(bad) }
    }
    let invalid = |bad| ApiError::invalid_owners(bad, owner_type);
    let missing = |all: &[String], found: &HashSet<String>| -> Vec<String> {
        all.iter()
            .filter(|id| !found.contains(*id))
            .cloned()
            .collect()
    };
    // Each owner as given, normalised to its canonical spelling for the lookups.
    let (owners, found): (Vec<Owner>, HashSet<String>) = match owner_type {
        OwnerType::Product => {
            let ids: Vec<ProductId> = parse_all(ids).map_err(invalid)?;
            let products = hypha_products::get_products(
                ctx,
                hypha_products::HasProducts::products(ports),
                &ids,
            )
            .await?;
            let found = products.iter().map(|p| p.id().to_string()).collect();
            (ids.into_iter().map(Owner::product).collect(), found)
        }
        OwnerType::Variant => {
            let ids: Vec<VariantId> = parse_all(ids).map_err(invalid)?;
            let products = hypha_products::products_of_variants(
                ctx,
                hypha_products::HasProducts::products(ports),
                &ids,
            )
            .await?;
            let product_of: HashMap<VariantId, ProductId> = products
                .iter()
                .flat_map(|p| p.variants().iter().map(move |v| (v.id(), p.id())))
                .collect();
            let found = product_of.keys().map(|v| v.to_string()).collect();
            let owners = ids
                .iter()
                .filter_map(|v| product_of.get(v).map(|p| Owner::variant(*p, *v)))
                .collect();
            (owners, found)
        }
        OwnerType::Category => {
            let ids: Vec<CategoryId> = parse_all(ids).map_err(invalid)?;
            let refs: Vec<Reference> = ids
                .iter()
                .map(|id| Reference {
                    kind: ReferenceKind::Category,
                    id: id.as_uuid(),
                })
                .collect();
            let existing = ports.existing(&refs).await?;
            let found = existing
                .iter()
                .map(|r| CategoryId::from_uuid(r.id).to_string())
                .collect();
            (ids.into_iter().map(Owner::category).collect(), found)
        }
        OwnerType::Customer => {
            let ids: Vec<CustomerId> = parse_all(ids).map_err(invalid)?;
            let refs: Vec<Reference> = ids
                .iter()
                .map(|id| Reference {
                    kind: ReferenceKind::Customer,
                    id: id.as_uuid(),
                })
                .collect();
            let existing = ports.existing(&refs).await?;
            let found = existing
                .iter()
                .map(|r| CustomerId::from_uuid(r.id).to_string())
                .collect();
            (ids.into_iter().map(Owner::customer).collect(), found)
        }
        OwnerType::Order => {
            let ids: Vec<OrderId> = parse_all(ids).map_err(invalid)?;
            let mut found = HashSet::new();
            let mut seen = HashSet::new();
            for id in &ids {
                // Orders have no batch read; a call holds at most 250.
                if seen.insert(*id)
                    && hypha_orders::OrderRepo::get(hypha_orders::HasOrders::orders(ports), *id)
                        .await?
                        .is_some()
                {
                    found.insert(id.to_string());
                }
            }
            (ids.into_iter().map(Owner::order).collect(), found)
        }
    };
    let canonical: Vec<String> = owners.iter().map(Owner::prefixed).collect();
    let all: Vec<String> = if owner_type == OwnerType::Variant {
        // Unknown variants have no owner yet: name them as sent.
        ids.iter()
            .filter(|id| {
                id.parse::<VariantId>()
                    .is_ok_and(|v| !found.contains(&v.to_string()))
            })
            .cloned()
            .collect()
    } else {
        missing(&canonical, &found)
    };
    if !all.is_empty() {
        return Err(ApiError::owners_not_found(all));
    }
    Ok(owners)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ListItemsRequest {
    /// 1 to 250 owners of the definition's owner type (`cus_…` for a customer field).
    owners: Vec<String>,
    /// Items to add where they're missing (each once; text ignoring case when the field
    /// has `unique_items`).
    #[serde(default)]
    #[schema(value_type = Vec<Object>)]
    add: Vec<Value>,
    /// Items to remove where they're present.
    #[serde(default)]
    #[schema(value_type = Vec<Object>)]
    remove: Vec<Value>,
}

/// One owner's list after the change.
#[derive(Serialize, utoipa::ToSchema)]
pub struct OwnerItemsResponse {
    owner: String,
    /// What this call added to its list (empty: nothing changed).
    #[schema(value_type = Vec<Object>)]
    added: Vec<Value>,
    #[schema(value_type = Vec<Object>)]
    removed: Vec<Value>,
    /// The value's version after; `null` when the list is empty (the value is gone).
    version: Option<u64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ListItemsResponse {
    /// In the call's order, each owner once.
    items: Vec<OwnerItemsResponse>,
}

/// Adds and removes items of one list field on 1 to 250 owners (design note 0033, CF6):
/// done in the database under each value's lock, so concurrent writers both land.
/// Adding what's there or removing what isn't changes nothing and records nothing. An
/// owner whose list would break `max_items`/`min_items` fails the whole call (`409
/// too_many_items` / `too_few_items`, naming it). Needs the field's owner type's write
/// scope.
#[utoipa::path(
    post,
    path = "/admin/custom-fields/{definition}/items",
    params(("definition" = CustomFieldDefinitionId, Path)),
    request_body = ListItemsRequest,
    responses((status = 200, description = "OK", body = ListItemsResponse)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn change_items<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(definition): PathId<CustomFieldDefinitionId>,
    body: JsonBody<ListItemsRequest>,
) -> Result<Json<ListItemsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    check_owner_count(req.owners.len())?;
    let owners = bulk_owners::<S>(&ports, &ctx, definition, &req.owners).await?;
    let kind = caller_kind(&ctx, &ports).await?;
    let caller = FieldCaller::new(&ctx, &kind);
    let type_of: HashMap<uuid::Uuid, OwnerType> =
        owners.iter().map(|o| (o.id(), o.kind())).collect();
    let applied =
        change_list_items(&caller, &ports, definition, owners, req.add, req.remove).await?;
    Ok(Json(ListItemsResponse {
        items: applied
            .into_iter()
            .map(|o| OwnerItemsResponse {
                owner: type_of
                    .get(&o.owner)
                    .map_or_else(|| o.owner.to_string(), |t| t.format_id(o.owner)),
                added: o.added,
                removed: o.removed,
                version: o.version.map(|v| v.value()),
            })
            .collect(),
    }))
}

/// One owner's new value.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OwnerValueRequest {
    owner: String,
    /// The value; `null` removes it.
    #[schema(value_type = serde_json::Value)]
    value: Value,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetFieldValuesRequest {
    /// 1 to 250 owners of the definition's owner type, each once.
    values: Vec<OwnerValueRequest>,
}

/// Sets (or, with `null`, removes) one field on 1 to 250 owners: imports, and "set on
/// selected". All or nothing: every problem is reported at once (`422
/// custom_fields_invalid`, each with its `owner`). Records one event per owner that
/// changed. Needs the field's owner type's write scope.
#[utoipa::path(
    post,
    path = "/admin/custom-fields/{definition}/values",
    params(("definition" = CustomFieldDefinitionId, Path)),
    request_body = SetFieldValuesRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn set_values<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(definition): PathId<CustomFieldDefinitionId>,
    body: JsonBody<SetFieldValuesRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    check_owner_count(req.values.len())?;
    let ids: Vec<String> = req.values.iter().map(|v| v.owner.clone()).collect();
    let owners = bulk_owners::<S>(&ports, &ctx, definition, &ids).await?;
    let kind = caller_kind(&ctx, &ports).await?;
    let caller = FieldCaller::new(&ctx, &kind);
    // `bulk_owners` keeps the body's order (and an unknown owner fails the call).
    let values = owners
        .into_iter()
        .zip(req.values)
        .map(|(owner, v)| (owner, (!v.value.is_null()).then_some(v.value)))
        .collect();
    set_field_values(&caller, &ports, definition, values).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 1 to 250 owners, before any of them is looked up.
fn check_owner_count(count: usize) -> Result<(), ApiError> {
    use hypha_custom_fields::{CommandError as E, limits::MAX_OWNERS_PER_CALL};
    match count {
        0 => Err(E::NoOwners.into()),
        n if n > MAX_OWNERS_PER_CALL => Err(E::TooManyOwners {
            max: MAX_OWNERS_PER_CALL,
        }
        .into()),
        _ => Ok(()),
    }
}
