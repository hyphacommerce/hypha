use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdError {
    #[error("expected prefix `{expected}_`")]
    WrongPrefix { expected: &'static str },
    #[error("invalid uuid")]
    InvalidUuid,
}

macro_rules! define_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(Uuid);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            pub fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", Self::PREFIX, self.0.simple())
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let rest = s
                    .strip_prefix(Self::PREFIX)
                    .and_then(|r| r.strip_prefix('_'))
                    .ok_or(IdError::WrongPrefix {
                        expected: Self::PREFIX,
                    })?;
                let uuid = Uuid::parse_str(rest).map_err(|_| IdError::InvalidUuid)?;
                Ok(Self(uuid))
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;

            fn try_from(s: String) -> Result<Self, Self::Error> {
                s.parse()
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.to_string()
            }
        }
    };
}

define_id!(ProductId, "prod");
define_id!(VariantId, "var");
define_id!(CartId, "cart");
define_id!(OptionId, "opt");
define_id!(OptionValueId, "optval");
define_id!(MarketId, "mkt");
define_id!(CatalogId, "cat");
define_id!(PriceListId, "pl");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_id_starts_with_prod() {
        let value = ProductId::from_uuid(Uuid::from_u128(1));
        assert!(value.to_string().starts_with("prod_"));
    }

    #[test]
    fn id_survives_round_trip() {
        let id = ProductId::from_uuid(Uuid::from_u128(52));
        let text = id.to_string();
        let parsed: ProductId = text.parse().unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn wrong_prefix_fails() {
        let var_id_text = VariantId::from_uuid(Uuid::from_u128(12)).to_string();
        let parsed = var_id_text.parse::<ProductId>();
        assert_eq!(parsed, Err(IdError::WrongPrefix { expected: "prod" }));
    }

    #[test]
    fn invalid_uuid_fails() {
        let parsed = "prod_nothex".parse::<ProductId>();
        assert_eq!(parsed, Err(IdError::InvalidUuid));
    }

    #[test]
    fn serializes_as_prefixed_string() {
        let id = ProductId::from_uuid(Uuid::from_u128(1));
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{}\"", id));
    }

    #[test]
    fn deserialize_rejects_wrong_prefix() {
        let json = format!("\"{}\"", VariantId::from_uuid(Uuid::from_u128(1)));
        assert!(serde_json::from_str::<ProductId>(&json).is_err())
    }
}
