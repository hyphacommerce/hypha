use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    #[serde(rename = "products:read")]
    ProductsRead,
    #[serde(rename = "products:write")]
    ProductsWrite,
    #[serde(rename = "markets:read")]
    MarketsRead,
    #[serde(rename = "markets:write")]
    MarketsWrite,
    #[serde(rename = "pricing:read")]
    PricingRead,
    #[serde(rename = "pricing:write")]
    PricingWrite,
    #[serde(rename = "carts:read")]
    CartsRead,
    #[serde(rename = "carts:write")]
    CartsWrite,
    #[serde(rename = "taxes:read")]
    TaxesRead,
    #[serde(rename = "taxes:write")]
    TaxesWrite,
}

impl Scope {
    /// The same string used in JSON, e.g. "products:write".
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::ProductsRead => "products:read",
            Scope::ProductsWrite => "products:write",
            Scope::MarketsRead => "markets:read",
            Scope::MarketsWrite => "markets:write",
            Scope::PricingRead => "pricing:read",
            Scope::PricingWrite => "pricing:write",
            Scope::CartsRead => "carts:read",
            Scope::CartsWrite => "carts:write",
            Scope::TaxesRead => "taxes:read",
            Scope::TaxesWrite => "taxes:write",
        }
    }

    pub const ALL: [Scope; 10] = [
        Scope::ProductsRead,
        Scope::ProductsWrite,
        Scope::MarketsRead,
        Scope::MarketsWrite,
        Scope::PricingRead,
        Scope::PricingWrite,
        Scope::CartsRead,
        Scope::CartsWrite,
        Scope::TaxesRead,
        Scope::TaxesWrite,
    ];
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn display_matches_json_for_every_scope() {
        for scope in Scope::ALL {
            assert_eq!(
                serde_json::to_string(&scope).unwrap(),
                format!("\"{scope}\"")
            );
        }
    }

    #[test]
    fn uses_colon_names_in_json() {
        assert_eq!(
            serde_json::to_string(&Scope::ProductsWrite).unwrap(),
            "\"products:write\""
        );
        assert_eq!(
            serde_json::from_str::<Scope>("\"markets:read\"").unwrap(),
            Scope::MarketsRead
        );
    }
}
