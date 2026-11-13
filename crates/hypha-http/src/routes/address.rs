//! A postal address on the wire, where it isn't an address-book entry: a location's
//! address, a cart's shipping address, the address an order snapshots. The customer
//! address book has its own DTOs (an ID and default flags beside these fields).

use hypha_core::{
    Address, AddressField, AddressParts, AddressPrecision, AddressVerdict, AddressVerification,
    CountryCode, FieldStatus, MessageParams, SuggestedAddress, VerificationFlags,
};
use serde::{Deserialize, Serialize};

use crate::{error::ApiError, routes::rfc3339};

/// A field of an address, as named in requests and responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AddressFieldDto {
    FirstName,
    LastName,
    Company,
    Line1,
    Line2,
    City,
    Province,
    PostalCode,
    Country,
    Phone,
}

impl From<AddressField> for AddressFieldDto {
    fn from(f: AddressField) -> Self {
        match f {
            AddressField::FirstName => Self::FirstName,
            AddressField::LastName => Self::LastName,
            AddressField::Company => Self::Company,
            AddressField::Line1 => Self::Line1,
            AddressField::Line2 => Self::Line2,
            AddressField::City => Self::City,
            AddressField::Province => Self::Province,
            AddressField::PostalCode => Self::PostalCode,
            AddressField::Country => Self::Country,
            AddressField::Phone => Self::Phone,
        }
    }
}

/// What a client needs to word a built-in cart message or an address problem from its
/// `code` in the shopper's language, rather than show the English `message`. Absent where
/// there's nothing to say: a checkout rule's or a validation app's message (worded by
/// whoever wrote it), a built-in one that needs no data, a message stored before params.
/// Fields are added, never renamed. By code:
///
/// - `required`, `unknown_region`: `field`.
/// - `too_long`: `field`, `max`.
/// - `invalid_format`: `field`, and `example` where the country gives one.
/// - `region_mismatch`: `field` (`postal_code`), `region` (the code the address names) and
///   `region_name`.
/// - `house_number_missing`: `field` (`line1`).
/// - `apartment_missing`: `field` (`line2`).
/// - `address_corrections_suggested`, `address_ambiguous`, `address_undeliverable`,
///   `validation_unavailable`: none (a suggested address is the cart's
///   `shipping_address_check.suggested`).
///
/// A field's label in the country's words is the address format's `label_keys`, but for
/// `country`, which forms show apart from the address's own fields.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct MessageParamsDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<AddressFieldDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = "90210")]
    example: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = "NY")]
    region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = "New York")]
    region_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<u32>,
}

impl MessageParamsDto {
    /// `None` for params with nothing in them: the field is left out.
    pub(crate) fn of(p: &MessageParams) -> Option<Self> {
        (!p.is_empty()).then(|| Self {
            field: p.field.map(AddressFieldDto::from),
            example: p.example.clone(),
            region: p.region.clone(),
            region_name: p.region_name.clone(),
            max: p.max,
        })
    }
}

/// Sent: last name, first line and city required, text trimmed, at most 255 characters
/// per field. A cart's or a location's address is also checked against its country's
/// format (`GET …/address-formats/{country}`): its required fields, the postal code's
/// shape, the region (a name, alternate name or code, stored as the code where the
/// country lists regions). Every problem comes back at once as `422 invalid_address`
/// with `details.problems: [{field, code, message, params}]` (`params` as a cart
/// message's).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PostalAddressRequest {
    #[serde(default)]
    first_name: Option<String>,
    last_name: String,
    #[serde(default)]
    company: Option<String>,
    line1: String,
    #[serde(default)]
    line2: Option<String>,
    city: String,
    #[serde(default)]
    province: Option<String>,
    #[serde(default)]
    postal_code: Option<String>,
    /// ISO 3166-1 alpha-2, e.g. `HR`.
    country: String,
    #[serde(default)]
    phone: Option<String>,
}

impl From<PostalAddressRequest> for AddressParts {
    fn from(r: PostalAddressRequest) -> Self {
        AddressParts {
            first_name: r.first_name,
            last_name: r.last_name,
            company: r.company,
            line1: r.line1,
            line2: r.line2,
            city: r.city,
            province: r.province,
            postal_code: r.postal_code,
            country: r.country,
            phone: r.phone,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct PostalAddressResponse {
    first_name: Option<String>,
    last_name: String,
    company: Option<String>,
    line1: String,
    line2: Option<String>,
    city: String,
    /// The region's code where the country lists regions (`CA`), else as entered.
    province: Option<String>,
    /// The region's name beside its code ("California"), where the country lists
    /// regions.
    province_name: Option<&'static str>,
    postal_code: Option<String>,
    country: CountryCode,
    phone: Option<String>,
    /// Personal fields left out for this caller: an API key or app without the matching
    /// `customer_data:*` scope. They are absent from the object then, whatever the schema
    /// says. Never sent to staff or storefronts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
}

/// The name of the address's region, where its country lists regions.
pub(crate) fn province_name(a: &Address) -> Option<&'static str> {
    a.parts()
        .province
        .as_deref()
        .and_then(|code| hypha_addresses::region_name(a.country(), code))
}

impl From<&Address> for PostalAddressResponse {
    fn from(a: &Address) -> Self {
        let p = a.parts();
        Self {
            first_name: p.first_name.clone(),
            last_name: p.last_name.clone(),
            company: p.company.clone(),
            line1: p.line1.clone(),
            line2: p.line2.clone(),
            city: p.city.clone(),
            province: p.province.clone(),
            province_name: province_name(a),
            postal_code: p.postal_code.clone(),
            country: a.country(),
            phone: p.phone.clone(),
            redacted: None,
        }
    }
}

/// An address a verification app suggests: where it is, without who lives there.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct SuggestedAddressResponse {
    company: Option<String>,
    line1: String,
    line2: Option<String>,
    city: String,
    /// The region's code where the country lists regions.
    province: Option<String>,
    province_name: Option<&'static str>,
    postal_code: Option<String>,
    country: String,
    /// Personal fields left out for this caller (`customer_data:address`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
}

impl From<&SuggestedAddress> for SuggestedAddressResponse {
    fn from(s: &SuggestedAddress) -> Self {
        let province_name = CountryCode::new(&s.country).ok().and_then(|country| {
            s.province
                .as_deref()
                .and_then(|code| hypha_addresses::region_name(country, code))
        });
        Self {
            company: s.company.clone(),
            line1: s.line1.clone(),
            line2: s.line2.clone(),
            city: s.city.clone(),
            province: s.province.clone(),
            province_name,
            postal_code: s.postal_code.clone(),
            country: s.country.clone(),
            redacted: None,
        }
    }
}

/// What the store's verification app said about a cart's shipping address. Show
/// `suggested` as "Did you mean …?": setting it is free (it's known deliverable), and
/// keeping the address as entered needs nothing recorded.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ShippingAddressCheckResponse {
    verdict: AddressVerdict,
    /// The corrected address, when the app is confident of it.
    suggested: Option<SuggestedAddressResponse>,
    flags: VerificationFlags,
    /// RFC 3339, UTC.
    checked_at: String,
}

impl ShippingAddressCheckResponse {
    pub(crate) fn new(v: &AddressVerification) -> Result<Self, ApiError> {
        Ok(Self {
            verdict: v.verdict,
            suggested: v.suggested.as_ref().map(SuggestedAddressResponse::from),
            flags: v.flags,
            checked_at: rfc3339(v.checked_at)?,
        })
    }
}

/// A verification as kept with an order, with what staff need to judge it.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct AddressVerificationResponse {
    verdict: AddressVerdict,
    precision: Option<AddressPrecision>,
    /// Per field (`postal_code`), how sure the provider is.
    #[schema(value_type = std::collections::HashMap<String, FieldStatus>)]
    fields: std::collections::BTreeMap<String, FieldStatus>,
    suggested: Option<SuggestedAddressResponse>,
    flags: VerificationFlags,
    /// The app that answered (or failed to).
    app: Option<String>,
    /// The provider's own ID for the lookup.
    reference: Option<String>,
    /// Why the verdict is `unknown`: the app couldn't be asked, or its answer was
    /// refused (its extension call log has the details).
    failure: Option<String>,
    /// RFC 3339, UTC.
    checked_at: String,
}

impl AddressVerificationResponse {
    pub(crate) fn new(v: &AddressVerification) -> Result<Self, ApiError> {
        Ok(Self {
            verdict: v.verdict,
            precision: v.precision,
            fields: v.fields.clone(),
            suggested: v.suggested.as_ref().map(SuggestedAddressResponse::from),
            flags: v.flags,
            app: v.app.as_ref().map(|a| a.as_str().to_string()),
            reference: v.reference.clone(),
            failure: v.failure.clone(),
            checked_at: rfc3339(v.checked_at)?,
        })
    }
}
