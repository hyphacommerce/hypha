//! OpenAPI schemas for core value types (design note 0010, S8), behind the `openapi`
//! feature. Validated newtypes serialize as plain strings or numbers, so their schemas
//! are written by hand here: the rules a client can check (pattern, range) are the ones
//! the constructors enforce.

use std::borrow::Cow;

use utoipa::{
    PartialSchema, ToSchema,
    openapi::{KnownFormat, ObjectBuilder, RefOr, Schema, SchemaFormat, Type, schema::SchemaType},
};

use crate::{BasisPoints, CountryCode, Email, Money, TaxRate};

/// A string schema with a pattern, an example and a description.
pub(crate) fn string_schema(pattern: &str, example: &str, description: &str) -> RefOr<Schema> {
    ObjectBuilder::new()
        .schema_type(SchemaType::new(Type::String))
        .pattern(Some(pattern))
        .examples([example])
        .description(Some(description))
        .into()
}

fn integer_schema(min: i64, max: i64, format: KnownFormat, description: &str) -> RefOr<Schema> {
    ObjectBuilder::new()
        .schema_type(SchemaType::new(Type::Integer))
        .format(Some(SchemaFormat::KnownFormat(format)))
        .minimum(Some(min))
        .maximum(Some(max))
        .description(Some(description))
        .into()
}

impl PartialSchema for CountryCode {
    fn schema() -> RefOr<Schema> {
        string_schema("^[A-Z]{2}$", "HR", "An ISO 3166-1 alpha-2 country code.")
    }
}

impl ToSchema for CountryCode {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("CountryCode")
    }
}

impl PartialSchema for Email {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(SchemaType::new(Type::String))
            .format(Some(SchemaFormat::KnownFormat(KnownFormat::Email)))
            .max_length(Some(Email::MAX_LEN))
            .examples(["ana@example.com"])
            .description(Some(
                "An email address; stored trimmed and lowercased, so responses show it that way.",
            ))
            .into()
    }
}

impl ToSchema for Email {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("Email")
    }
}

impl PartialSchema for TaxRate {
    fn schema() -> RefOr<Schema> {
        integer_schema(
            0,
            i64::from(TaxRate::MAX),
            KnownFormat::Int32,
            "A tax rate in parts per million: 250000 is 25 %.",
        )
    }
}

impl ToSchema for TaxRate {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("TaxRate")
    }
}

impl PartialSchema for BasisPoints {
    fn schema() -> RefOr<Schema> {
        integer_schema(
            i64::from(BasisPoints::MIN),
            i64::from(i32::MAX),
            KnownFormat::Int32,
            "A percentage in basis points: 500 is 5 %, -1000 is -10 %.",
        )
    }
}

impl ToSchema for BasisPoints {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("BasisPoints")
    }
}

impl PartialSchema for Money {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(SchemaType::new(Type::Object))
            .property(
                "amount",
                integer_schema(
                    -Money::MAX_AMOUNT,
                    Money::MAX_AMOUNT,
                    KnownFormat::Int64,
                    "In minor units (1050 is 10.50). At most 2^53 - 1 either way, so every \
                     amount is exact in JSON.",
                ),
            )
            .required("amount")
            .property("currency", crate::Currency::schema())
            .required("currency")
            .examples([serde_json::json!({ "amount": 1050, "currency": "EUR" })])
            .description(Some("An amount of money in one currency."))
            .into()
    }
}

impl ToSchema for Money {
    fn name() -> Cow<'static, str> {
        Cow::Borrowed("Money")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use uuid::Uuid;

    use super::*;
    use crate::{Currency, ProductId, Scope};

    fn json_of<T: PartialSchema>() -> Value {
        serde_json::to_value(T::schema()).unwrap()
    }

    #[test]
    fn an_id_pattern_matches_how_ids_serialize() {
        let schema = json_of::<ProductId>();
        assert_eq!(schema["pattern"], "^prod_[0-9a-f]{32}$");
        let id = ProductId::from_uuid(Uuid::from_u128(7)).to_string();
        assert!(id.starts_with("prod_") && id.len() == "prod_".len() + 32);
    }

    #[test]
    fn money_is_an_object_with_amount_and_currency() {
        let schema = json_of::<Money>();
        assert_eq!(schema["required"], json!(["amount", "currency"]));
        assert_eq!(
            schema["properties"]["amount"]["maximum"],
            json!(Money::MAX_AMOUNT)
        );
        let money = serde_json::to_value(Money::new(1050, Currency::Eur).unwrap()).unwrap();
        assert_eq!(money, json!({ "amount": 1050, "currency": "EUR" }));
    }

    #[test]
    fn enums_list_exactly_their_json_names() {
        let currencies = json_of::<Currency>();
        let names: Vec<String> = [Currency::Eur, Currency::Usd, Currency::Rsd]
            .iter()
            .map(|c| {
                serde_json::to_value(c)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(currencies["enum"], json!(names));

        let scopes = json_of::<Scope>();
        let names: Vec<&str> = Scope::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(scopes["enum"], json!(names));
    }
}
