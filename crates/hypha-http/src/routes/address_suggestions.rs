//! Address autocomplete: suggestions while someone types an address, then the picked one's
//! parts, from the store's autocomplete app. Through the API so the provider's keys stay
//! with the app; rate limited per client address and per typing session, so nobody can
//! spend the merchant's lookups.

use axum::{Json, extract::State};
use hypha_addresses::Formats;
use hypha_core::{
    Address, AddressField, AddressFormats, AddressParts, CountryCode, Ctx, Scope, SuggestedAddress,
};
use hypha_extensions::{
    AddressResolveRequest, AddressSuggestRequest, AddressSuggestion, MAX_SUGGESTION_ID,
    MAX_SUGGESTIONS,
};
use serde::{Deserialize, Serialize};

use crate::{
    client_ip::ClientIp,
    error::ApiError,
    extract::{Caller, PathId, Query, Storefront},
    rate_limit::{self, Rule},
    routes::address::{AddressFieldDto, MessageParamsDto, SuggestedAddressResponse},
    state::{SharedState, Storage},
};

/// The shortest and longest `q`, in characters: fewer than three letters match too much
/// to be worth a lookup.
const QUERY_CHARS: std::ops::RangeInclusive<usize> = 3..=200;

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct SuggestParams {
    /// What's typed so far, 3 to 200 characters.
    q: String,
    /// The country the form is for (`HR`): suggestions stay in it.
    country: String,
    /// A token the form makes for one address being typed and keeps until one is picked
    /// (1 to 512 letters, digits, `-` and `_`): providers bill per session, and the rate
    /// limit counts per session.
    session: String,
    /// At most this many suggestions (1 to 5, 5 by default).
    limit: Option<u8>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ResolveParams {
    /// The country the form is for: the address must be in it.
    country: String,
    /// The session the suggestion was offered in.
    session: String,
}

/// What to show under the field.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AddressSuggestionsResponse {
    /// At most `limit`; none when the app couldn't answer (the person types on).
    suggestions: Vec<AddressSuggestionResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AddressSuggestionResponse {
    /// Send it to `GET …/address-suggestions/{id}` when this one is picked.
    id: String,
    /// The main line ("Ilica 1").
    label: String,
    /// The rest, shown smaller ("10000 Zagreb, Croatia").
    description: Option<String>,
}

impl From<AddressSuggestion> for AddressSuggestionResponse {
    fn from(s: AddressSuggestion) -> Self {
        Self {
            id: s.id,
            label: s.label,
            description: s.description,
        }
    }
}

/// One thing about the resolved address the country's format check doesn't accept: the
/// form fills it in anyway and shows this under the field.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ResolvedProblem {
    field: AddressFieldDto,
    /// `required`, `too_long`, `invalid_format`, `unknown_region` or `region_mismatch`,
    /// as `invalid_address`'s problems.
    code: &'static str,
    message: String,
    params: MessageParamsDto,
}

/// The picked suggestion's address, normalised like any address a form sends (the region
/// as its code, the postal code upper-cased), to fill the form with. Names and phone
/// aren't part of it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ResolvedAddressResponse {
    address: SuggestedAddressResponse,
    /// What saving it as it is would refuse (a missing postal code), so the form can say
    /// so before the person saves. Empty when it passes.
    problems: Vec<ResolvedProblem>,
}

/// `session` as the parameters' docs say.
fn session(raw: &str) -> Result<&str, ApiError> {
    let fine = !raw.is_empty()
        && raw.len() <= MAX_SUGGESTION_ID
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if fine {
        Ok(raw)
    } else {
        Err(ApiError::invalid_suggestion_query(
            "session",
            format!("session is 1 to {MAX_SUGGESTION_ID} letters, digits, `-` and `_`"),
        ))
    }
}

/// Checked before anything is asked: the caller may, and isn't over the limits.
async fn admit<S: Storage>(
    state: &SharedState<S>,
    ctx: &Ctx,
    scope: Scope,
    ip: &str,
    session_key: &str,
) -> Result<(), ApiError> {
    ctx.require(scope)?;
    rate_limit::enforce(&state.limiter, Rule::AddressSuggestIp, ip).await?;
    rate_limit::enforce(&state.limiter, Rule::AddressSuggestSession, session_key).await
}

async fn suggestions<'a, S: Storage>(
    state: &'a SharedState<S>,
    caller_ports: (S::Ports<'a>, Ctx),
    scope: Scope,
    ip: &str,
    query: Query<SuggestParams>,
) -> Result<Json<AddressSuggestionsResponse>, ApiError> {
    let (ports, ctx) = caller_ports;
    let p = query.parse()?;
    let session = session(&p.session)?;
    admit(state, &ctx, scope, ip, session).await?;
    let typed = p.q.trim();
    if !QUERY_CHARS.contains(&typed.chars().count()) {
        return Err(ApiError::invalid_suggestion_query(
            "q",
            format!(
                "q is {} to {} characters",
                QUERY_CHARS.start(),
                QUERY_CHARS.end()
            ),
        ));
    }
    let limit = p.limit.unwrap_or(MAX_SUGGESTIONS);
    if !(1..=MAX_SUGGESTIONS).contains(&limit) {
        return Err(ApiError::invalid_suggestion_query(
            "limit",
            format!("limit is 1 to {MAX_SUGGESTIONS}"),
        ));
    }
    let country = CountryCode::new(&p.country)?;
    let autocomplete = state.autocomplete(&ports);
    let app = autocomplete
        .app()
        .await?
        .ok_or_else(ApiError::autocomplete_unavailable)?;
    let found = autocomplete
        .suggest(
            &app,
            &AddressSuggestRequest {
                query: typed.to_string(),
                country,
                session: session.to_string(),
                limit,
            },
        )
        .await?;
    Ok(Json(AddressSuggestionsResponse {
        suggestions: found.into_iter().map(Into::into).collect(),
    }))
}

async fn resolved<'a, S: Storage>(
    state: &'a SharedState<S>,
    caller_ports: (S::Ports<'a>, Ctx),
    scope: Scope,
    ip: &str,
    id: String,
    query: Query<ResolveParams>,
) -> Result<Json<ResolvedAddressResponse>, ApiError> {
    let (ports, ctx) = caller_ports;
    let p = query.parse()?;
    let session = session(&p.session)?;
    admit(state, &ctx, scope, ip, session).await?;
    if id.is_empty() || id.len() > MAX_SUGGESTION_ID {
        return Err(ApiError::invalid_suggestion_query(
            "id",
            format!("a suggestion's id is 1 to {MAX_SUGGESTION_ID} bytes"),
        ));
    }
    let country = CountryCode::new(&p.country)?;
    let autocomplete = state.autocomplete(&ports);
    let app = autocomplete
        .app()
        .await?
        .ok_or_else(ApiError::autocomplete_unavailable)?;
    let address = autocomplete
        .resolve(
            &app,
            &AddressResolveRequest {
                id,
                country,
                session: session.to_string(),
            },
        )
        .await??;
    Ok(Json(checked(address)))
}

/// The resolved address through the country's format: normalised, with what a write would
/// refuse. An address that isn't one at all (a field over its length) comes back as the
/// app gave it, with the problems `Address::new` finds.
fn checked(s: SuggestedAddress) -> ResolvedAddressResponse {
    let parts = AddressParts {
        first_name: None,
        // Names aren't part of a resolved address; one is needed to check it.
        last_name: "-".to_string(),
        company: s.company.clone(),
        line1: s.line1.clone(),
        line2: s.line2.clone(),
        city: s.city.clone(),
        province: s.province.clone(),
        postal_code: s.postal_code.clone(),
        country: s.country.to_ascii_uppercase(),
        phone: None,
    };
    let (address, problems) = match Formats.validate(parts.clone()) {
        Ok(checked) => (from_address(&checked.address), Vec::new()),
        Err(e) => {
            let normalised = Address::new(parts)
                .map(|a| from_address(&Formats.check(&a).address))
                .unwrap_or_else(|_| s.clone());
            (normalised, e.problems())
        }
    };
    ResolvedAddressResponse {
        address: SuggestedAddressResponse::from(&address),
        problems: problems
            .into_iter()
            .filter(|p| {
                !matches!(
                    p.field,
                    AddressField::FirstName | AddressField::LastName | AddressField::Phone
                )
            })
            .map(|p| ResolvedProblem {
                field: p.field.into(),
                code: p.code.as_str(),
                params: MessageParamsDto::from(&p.params),
                message: p.message,
            })
            .collect(),
    }
}

fn from_address(a: &Address) -> SuggestedAddress {
    let p = a.parts();
    SuggestedAddress {
        company: p.company.clone(),
        line1: p.line1.clone(),
        line2: p.line2.clone(),
        city: p.city.clone(),
        province: p.province.clone(),
        postal_code: p.postal_code.clone(),
        country: p.country.clone(),
    }
}

/// Addresses for what a shopper has typed so far, from the store's autocomplete app.
/// Debounce keystrokes; `404 autocomplete_unavailable` without an app (the address
/// format's `autocomplete` says so beforehand); no suggestions when the app fails.
#[utoipa::path(
    get,
    path = "/storefront/address-suggestions",
    params(SuggestParams),
    responses(
        (status = 200, description = "OK", body = AddressSuggestionsResponse),
        (status = 404, description = "The store has no autocomplete app (`autocomplete_unavailable`)", body = crate::error::ApiErrorBody),
        (status = 422, description = "`q`, `session` or `limit` breaks its rule (`invalid_suggestion_query`)", body = crate::error::ApiErrorBody),
        (status = 429, description = "Over the rate limit per client address or per session (`rate_limited`); see `Retry-After`", body = crate::error::ApiErrorBody),
    ),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn storefront_suggest<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    caller: Caller<Storefront>,
    query: Query<SuggestParams>,
) -> Result<Json<AddressSuggestionsResponse>, ApiError> {
    let found = caller.authenticate(&state).await?;
    suggestions(&state, found, Scope::CartsWrite, &ip, query).await
}

/// The address a shopper picked, normalised, with what saving it would refuse. `502
/// app_unavailable` when the app can't give it: the shopper types it instead.
#[utoipa::path(
    get,
    path = "/storefront/address-suggestions/{id}",
    params(("id" = String, Path, example = "ilica-1", description = "The suggestion's `id`."), ResolveParams),
    responses(
        (status = 200, description = "OK", body = ResolvedAddressResponse),
        (status = 404, description = "The store has no autocomplete app (`autocomplete_unavailable`)", body = crate::error::ApiErrorBody),
        (status = 429, description = "Over the rate limit per client address or per session (`rate_limited`); see `Retry-After`", body = crate::error::ApiErrorBody),
        (status = 502, description = "The app couldn't give the address (`app_unavailable`)", body = crate::error::ApiErrorBody),
    ),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn storefront_resolve<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    caller: Caller<Storefront>,
    PathId(id): PathId<String>,
    query: Query<ResolveParams>,
) -> Result<Json<ResolvedAddressResponse>, ApiError> {
    let found = caller.authenticate(&state).await?;
    resolved(&state, found, Scope::CartsWrite, &ip, id, query).await
}

/// Addresses for what staff have typed so far, from the store's autocomplete app, as the
/// storefront's.
#[utoipa::path(
    get,
    path = "/admin/address-suggestions",
    params(SuggestParams),
    responses(
        (status = 200, description = "OK", body = AddressSuggestionsResponse),
        (status = 404, description = "The store has no autocomplete app (`autocomplete_unavailable`)", body = crate::error::ApiErrorBody),
        (status = 422, description = "`q`, `session` or `limit` breaks its rule (`invalid_suggestion_query`)", body = crate::error::ApiErrorBody),
        (status = 429, description = "Over the rate limit per client address or per session (`rate_limited`); see `Retry-After`", body = crate::error::ApiErrorBody),
    ),
    extensions(("x-hypha-scopes" = json!(["customers:read"]))),
)]
pub async fn admin_suggest<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    caller: Caller,
    query: Query<SuggestParams>,
) -> Result<Json<AddressSuggestionsResponse>, ApiError> {
    let found = caller.authenticate(&state).await?;
    suggestions(&state, found, Scope::CustomersRead, &ip, query).await
}

/// The address staff picked, normalised, with what saving it would refuse.
#[utoipa::path(
    get,
    path = "/admin/address-suggestions/{id}",
    params(("id" = String, Path, example = "ilica-1", description = "The suggestion's `id`."), ResolveParams),
    responses(
        (status = 200, description = "OK", body = ResolvedAddressResponse),
        (status = 404, description = "The store has no autocomplete app (`autocomplete_unavailable`)", body = crate::error::ApiErrorBody),
        (status = 429, description = "Over the rate limit per client address or per session (`rate_limited`); see `Retry-After`", body = crate::error::ApiErrorBody),
        (status = 502, description = "The app couldn't give the address (`app_unavailable`)", body = crate::error::ApiErrorBody),
    ),
    extensions(("x-hypha-scopes" = json!(["customers:read"]))),
)]
pub async fn admin_resolve<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    caller: Caller,
    PathId(id): PathId<String>,
    query: Query<ResolveParams>,
) -> Result<Json<ResolvedAddressResponse>, ApiError> {
    let found = caller.authenticate(&state).await?;
    resolved(&state, found, Scope::CustomersRead, &ip, id, query).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_of_letters_digits_dashes_and_underscores_passes() {
        assert_eq!(session("a1-B_2").ok(), Some("a1-B_2"));
    }

    #[test]
    fn an_empty_session_is_refused() {
        assert!(session("").is_err());
    }

    #[test]
    fn a_session_with_a_space_is_refused() {
        assert!(session("a b").is_err());
    }

    #[test]
    fn a_resolved_address_is_normalised() {
        let r = checked(SuggestedAddress {
            company: None,
            line1: "1600 Amphitheatre Pkwy".to_string(),
            line2: None,
            city: "Mountain View".to_string(),
            province: Some("california".to_string()),
            postal_code: Some("94043".to_string()),
            country: "US".to_string(),
        });
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["address"]["province"], "CA");
        assert_eq!(json["address"]["province_name"], "California");
        assert_eq!(json["problems"], serde_json::json!([]));
    }

    #[test]
    fn a_resolved_address_missing_a_required_field_says_so() {
        let r = checked(SuggestedAddress {
            company: None,
            line1: "Ilica 1".to_string(),
            line2: None,
            city: "Zagreb".to_string(),
            province: None,
            postal_code: None,
            country: "HR".to_string(),
        });
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["address"]["line1"], "Ilica 1");
        assert_eq!(json["problems"][0]["field"], "postal_code");
        assert_eq!(json["problems"][0]["code"], "required");
    }
}
