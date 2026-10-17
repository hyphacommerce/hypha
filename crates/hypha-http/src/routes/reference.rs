//! Reference data for pickers (design note 0021, SU3): core's tables as the API accepts
//! them. Anyone with admin credentials may read them; they hold nothing about the store.

use axum::{Json, extract::State};
use hypha_core::{CountryCode, Currency};
use serde::Serialize;

use crate::{
    error::ApiError,
    extract::Caller,
    state::{SharedState, Storage},
};

#[derive(Serialize, utoipa::ToSchema)]
pub struct CurrencyResponse {
    code: Currency,
    /// The English name ("Japanese Yen"). UIs show their own language's name
    /// (`Intl.DisplayNames`) and fall back to this.
    name: &'static str,
    /// Digits after the decimal point (0 for JPY, 2 for EUR, 3 for KWD). Amounts are
    /// always integers in minor units: 1050 is 10.50 EUR but 1050 JPY.
    #[schema(minimum = 0)]
    minor_units: u8,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CurrencyListResponse {
    /// Sorted by code. A fixed list of about 150, so it comes in one response.
    items: Vec<CurrencyResponse>,
}

/// Every currency a market or price can use, with its minor units.
#[utoipa::path(
    get,
    path = "/admin/currencies",
    responses((status = 200, description = "OK", body = CurrencyListResponse)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn currencies<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<CurrencyListResponse>, ApiError> {
    caller.authenticate(&state).await?;
    Ok(Json(CurrencyListResponse {
        items: Currency::all()
            .map(|code| CurrencyResponse {
                code,
                name: code.name(),
                minor_units: code.minor_units(),
            })
            .collect(),
    }))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CountryResponse {
    code: CountryCode,
    /// The English name ("South Korea"). UIs show their own language's name
    /// (`Intl.DisplayNames`) and fall back to this.
    name: &'static str,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CountryListResponse {
    /// Sorted by code: the assigned ISO 3166-1 codes, so it comes in one response.
    items: Vec<CountryResponse>,
}

/// Every country a market, address or tax region can name.
#[utoipa::path(
    get,
    path = "/admin/countries",
    responses((status = 200, description = "OK", body = CountryListResponse)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn countries<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<CountryListResponse>, ApiError> {
    caller.authenticate(&state).await?;
    Ok(Json(CountryListResponse {
        items: CountryCode::all()
            .map(|code| CountryResponse {
                code,
                name: code.name(),
            })
            .collect(),
    }))
}
