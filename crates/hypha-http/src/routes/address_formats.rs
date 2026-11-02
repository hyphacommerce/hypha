//! Each country's address form: what a storefront's or the admin's form shows, in the
//! country's order, with its labels and regions. Public data, the same for every store.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use hypha_addresses::{Form, PostalCode};
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
    /// Each field's label, by field ("State", "ZIP code"). English for now.
    #[schema(value_type = HashMap<String, String>)]
    labels: BTreeMap<&'static str, &'static str>,
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
