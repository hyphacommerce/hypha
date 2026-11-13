//! Each country's address form: what a storefront's or the admin's form shows, in the
//! country's order, with its labels and regions. Public data, the same for every store.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use hypha_addresses::{Form, LabelKey, PostalCode};
use hypha_checkout_rules::{CheckoutSettingsRepo, HasCheckoutSettings};
use hypha_core::{AddressField, CountryCode, RepoError};
use serde::Serialize;

use crate::{
    error::ApiError,
    extract::{Caller, PathId, Storefront},
    routes::address::AddressFieldDto,
    state::{SharedState, Storage},
};

/// How a country uses postal codes.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PostalCodeUse {
    /// The country has none: the form doesn't ask, and one sent is dropped.
    None,
    Required,
    /// Asked for, but an address without one is accepted.
    Recommended,
    Optional,
}

/// What a field's label says, by meaning: a closed set for a client to translate labels
/// by, the English `labels` being one rendering of it. Values are added, never renamed.
#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LabelKeyDto {
    FirstName,
    LastName,
    Company,
    /// "Address".
    Address1,
    /// "Apartment, suite, etc.".
    Address2,
    StreetAndHouseNumber,
    FullAddress,
    AdditionalAddress,
    City,
    /// "City/town".
    CityTown,
    /// "City/ward/town/village".
    CityWardTownVillage,
    Ward,
    District,
    Suburb,
    Area,
    /// "Region": a region's label where the country names it nothing else.
    Region,
    Province,
    State,
    /// "State/territory".
    StateTerritory,
    County,
    Prefecture,
    Emirate,
    Governorate,
    Department,
    /// "Postal code".
    PostalCode,
    Postcode,
    /// "ZIP code".
    ZipCode,
    /// "PIN code".
    PinCode,
    Phone,
}

impl From<LabelKey> for LabelKeyDto {
    fn from(key: LabelKey) -> Self {
        match key {
            LabelKey::FirstName => Self::FirstName,
            LabelKey::LastName => Self::LastName,
            LabelKey::Company => Self::Company,
            LabelKey::Address1 => Self::Address1,
            LabelKey::Address2 => Self::Address2,
            LabelKey::StreetAndHouseNumber => Self::StreetAndHouseNumber,
            LabelKey::FullAddress => Self::FullAddress,
            LabelKey::AdditionalAddress => Self::AdditionalAddress,
            LabelKey::City => Self::City,
            LabelKey::CityTown => Self::CityTown,
            LabelKey::CityWardTownVillage => Self::CityWardTownVillage,
            LabelKey::Ward => Self::Ward,
            LabelKey::District => Self::District,
            LabelKey::Suburb => Self::Suburb,
            LabelKey::Area => Self::Area,
            LabelKey::Region => Self::Region,
            LabelKey::Province => Self::Province,
            LabelKey::State => Self::State,
            LabelKey::StateTerritory => Self::StateTerritory,
            LabelKey::County => Self::County,
            LabelKey::Prefecture => Self::Prefecture,
            LabelKey::Emirate => Self::Emirate,
            LabelKey::Governorate => Self::Governorate,
            LabelKey::Department => Self::Department,
            LabelKey::PostalCode => Self::PostalCode,
            LabelKey::Postcode => Self::Postcode,
            LabelKey::ZipCode => Self::ZipCode,
            LabelKey::PinCode => Self::PinCode,
            LabelKey::Phone => Self::Phone,
        }
    }
}

/// A label's key, or `null` for a label outside the set.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(transparent)]
pub struct LabelKeyOrNull(Option<LabelKeyDto>);

#[derive(Serialize, utoipa::ToSchema)]
pub struct RegionResponse {
    /// What `province` holds: `CA`, `M`.
    code: &'static str,
    /// The English name: "California", "Madrid".
    name: &'static str,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AddressFormatResponse {
    country: CountryCode,
    /// The form's fields, row by row, in the country's order; the country itself apart.
    rows: Vec<Vec<AddressFieldDto>>,
    /// The fields a write refuses empty (`422 invalid_address`, `required`).
    required: Vec<AddressFieldDto>,
    /// Each field's label, by field ("State", "ZIP code"), in English.
    #[schema(value_type = HashMap<String, String>)]
    labels: BTreeMap<&'static str, &'static str>,
    /// Each label's key, by field, for the same fields as `labels` (`province: "state"`,
    /// `postal_code: "zip_code"`): what a form in another language translates the label
    /// by. `null` for a label outside the set, which is then shown as `labels` has it.
    #[schema(value_type = HashMap<String, LabelKeyOrNull>)]
    label_keys: BTreeMap<&'static str, LabelKeyOrNull>,
    postal_code: PostalCodeUse,
    /// A postal code in the country's shape, for a placeholder.
    postal_code_example: Option<&'static str>,
    /// The regions to choose from, where the country lists them: the region is then a
    /// select and `province` holds the code. Empty where it's free text or not asked.
    regions: Vec<RegionResponse>,
    /// Whether the first line is expected to hold a house number.
    house_number_required: bool,
    /// Whether the store has an address autocomplete app: offer suggestions on the first
    /// line through `…/address-suggestions`.
    autocomplete: bool,
}

impl From<Form> for AddressFormatResponse {
    fn from(form: Form) -> Self {
        let field = |f: &AddressField| AddressFieldDto::from(*f);
        Self {
            country: form.country(),
            rows: form
                .rows()
                .iter()
                .map(|row| row.iter().map(field).collect())
                .collect(),
            required: form.required().iter().map(field).collect(),
            labels: form
                .labels()
                .into_iter()
                .map(|(f, label)| (f.as_str(), label))
                .collect(),
            label_keys: form
                .label_keys()
                .into_iter()
                .map(|(f, key)| (f.as_str(), LabelKeyOrNull(key.map(LabelKeyDto::from))))
                .collect(),
            postal_code: match form.postal_code() {
                PostalCode::None => PostalCodeUse::None,
                PostalCode::Required => PostalCodeUse::Required,
                PostalCode::Recommended => PostalCodeUse::Recommended,
                PostalCode::Optional => PostalCodeUse::Optional,
            },
            postal_code_example: form.postal_code_example(),
            regions: form
                .regions()
                .map(|r| RegionResponse {
                    code: r.code,
                    name: r.name,
                })
                .collect(),
            house_number_required: form.house_number_required(),
            autocomplete: false,
        }
    }
}

/// Whether the store names an autocomplete app.
async fn autocomplete(ports: &impl HasCheckoutSettings) -> Result<bool, RepoError> {
    let settings = ports.checkout_settings().get().await?;
    Ok(settings.is_some_and(|s| s.autocomplete_app().is_some()))
}

fn format_of(country: &str, autocomplete: bool) -> Result<Response, ApiError> {
    let country = CountryCode::new(country).map_err(|e| {
        ApiError::from(e)
            .not_found_if("unknown_country_code")
            .not_found_if("invalid_country_code")
    })?;
    let mut form = AddressFormatResponse::from(Form::of(country));
    form.autocomplete = autocomplete;
    let mut response = Json(form).into_response();
    // The form changes only with a release; `autocomplete` is the store's, and follows a
    // change of its settings within minutes.
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    Ok(response)
}

/// A country's address form: its fields in order, required ones, labels, regions.
#[utoipa::path(
    get,
    path = "/admin/address-formats/{country}",
    params(("country" = CountryCode, Path)),
    responses((status = 200, description = "OK", body = AddressFormatResponse)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn admin<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(country): PathId<String>,
) -> Result<Response, ApiError> {
    let (ports, _) = caller.authenticate(&state).await?;
    format_of(&country, autocomplete(&ports).await?)
}

/// A country's address form: its fields in order, required ones, labels, regions.
#[utoipa::path(
    get,
    path = "/storefront/address-formats/{country}",
    params(("country" = CountryCode, Path)),
    responses((status = 200, description = "OK", body = AddressFormatResponse)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn storefront<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(country): PathId<String>,
) -> Result<Response, ApiError> {
    let (ports, _) = caller.authenticate(&state).await?;
    format_of(&country, autocomplete(&ports).await?)
}
