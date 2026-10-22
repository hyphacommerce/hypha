use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum IdError {
    #[error("expected prefix `{expected}_`")]
    WrongPrefix { expected: &'static str },
    #[error("invalid uuid")]
    InvalidUuid,
}

impl crate::ErrorCode for IdError {
    fn code(&self) -> &'static str {
        match self {
            IdError::WrongPrefix { .. } | IdError::InvalidUuid => "invalid_id",
        }
    }
}

macro_rules! define_id {
    ($name:ident, $prefix:literal) => {
        /// Ordered like the UUID inside: by creation time for UUIDv7 (list pages).
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
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

        /// A string: the prefix, an underscore and 32 lowercase hex digits.
        #[cfg(feature = "openapi")]
        impl utoipa::PartialSchema for $name {
            fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::Schema> {
                crate::openapi::string_schema(
                    concat!("^", $prefix, "_[0-9a-f]{32}$"),
                    concat!($prefix, "_0190f1a2b3c47d8e9f00112233445566"),
                    concat!(
                        "A ",
                        stringify!($name),
                        ": `",
                        $prefix,
                        "_` and 32 hex digits."
                    ),
                )
            }
        }

        #[cfg(feature = "openapi")]
        impl utoipa::ToSchema for $name {
            fn name() -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed(stringify!($name))
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
define_id!(UserId, "user");
define_id!(AgentId, "agent");
// A write an agent proposed, waiting for a person.
define_id!(ProposalId, "prop");
// One tool call an agent made, in its call log.
define_id!(AgentCallId, "acall");
define_id!(StoreId, "store");

define_id!(TaxClassId, "txc");
define_id!(TaxRegionId, "txreg");

define_id!(LocationId, "loc");
define_id!(InventoryItemId, "invi");

define_id!(CustomerId, "cus");
define_id!(CustomerGroupId, "cgrp");
define_id!(AddressId, "addr");

define_id!(OrderId, "ord");
define_id!(OrderLineId, "ordl");
define_id!(FulfillmentId, "ful");
// Payment methods and payments.
define_id!(PaymentMethodId, "pm");
define_id!(PaymentId, "pay");
define_id!(ChannelId, "chan");
// Categories.
define_id!(CategoryId, "pcat");

define_id!(RoleId, "role");
define_id!(InviteId, "inv");
define_id!(IdentityId, "idn");
define_id!(SessionId, "sess");
define_id!(ApiKeyId, "key");
define_id!(PasswordResetId, "pwr");

define_id!(AssetId, "asset");

define_id!(ShippingZoneId, "szone");
define_id!(ShippingOptionId, "shipopt");
define_id!(ShippingGroupId, "shgrp");

// Outbox events and their deliveries: IDs the admin and webhooks show.
define_id!(EventId, "evt");
define_id!(DeliveryId, "dlv");
// Emails sent and their records.
define_id!(NotificationId, "ntf");
// Calls Hypha made to apps' extension points.
define_id!(ExtensionCallId, "xcall");
// Apps installed through OAuth act as `Actor::App(AppId)`: a Copy identity beside the app's
// key (`app_<name>`), whose prefix can't be mistaken for a key.
define_id!(AppId, "appid");
// Pending OAuth authorization requests, shown on the consent page.
define_id!(OAuthRequestId, "oareq");
// Custom field definitions.
define_id!(CustomFieldDefinitionId, "cfd");

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
