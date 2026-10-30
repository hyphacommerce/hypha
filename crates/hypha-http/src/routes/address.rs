//! A postal address on the wire, where it isn't an address-book entry: a location's
//! address, a cart's shipping address, the address an order snapshots. The customer
//! address book has its own DTOs (an ID and default flags beside these fields).

use hypha_core::{Address, AddressParts, CountryCode};
use serde::{Deserialize, Serialize};

/// Sent: checked by `Address::new` (last name, first line and city required; text is
/// trimmed, at most 255 characters per field).
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
    province: Option<String>,
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
            postal_code: p.postal_code.clone(),
            country: a.country(),
            phone: p.phone.clone(),
            redacted: None,
        }
    }
}
