//! A postal address on the wire, where it isn't an address-book entry: a location's
//! address, a cart's shipping address, the address an order snapshots. The customer
//! address book has its own DTOs (an ID and default flags beside these fields).

use hypha_core::AddressParts;
use serde::Deserialize;

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

/// Received: the extension points' postal address (`shipping.rates` sends it too), so
/// the APIs and the calls to apps spell an address one way. Built `From<&Address>`.
pub type PostalAddressResponse = hypha_extensions::PostalAddress;
