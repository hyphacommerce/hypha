//! Field helpers for PATCH bodies, which follow JSON Merge Patch (RFC 7396): a field
//! that is absent keeps its value, `null` clears it, anything else sets it.
//!
//! serde can't tell an absent field from `null` on its own (both become `None`), so:
//! - a field that can be cleared is `Option<Option<T>>` with
//!   `#[serde(default, deserialize_with = "patch::nullable")]`: absent → `None`,
//!   `null` → `Some(None)`, a value → `Some(Some(v))`;
//! - a field that can't be cleared is `Option<T>` with
//!   `#[serde(default, deserialize_with = "patch::non_null")]`: absent → `None`, and
//!   `null` is rejected (`invalid_body`) instead of being quietly ignored.

use serde::{Deserialize, Deserializer};

/// Only called when the field is present, so `null` here means "clear".
pub fn nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// Only called when the field is present; `T` itself rejects `null`.
pub fn non_null<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d).map(Some)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Body {
        #[serde(default, deserialize_with = "super::nullable")]
        phone: Option<Option<String>>,
        #[serde(default, deserialize_with = "super::non_null")]
        tracked: Option<bool>,
    }

    fn parse(json: &str) -> Result<Body, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn absent_null_and_value_are_three_different_things() {
        let absent = parse("{}").unwrap();
        assert_eq!((absent.phone, absent.tracked), (None, None));
        assert_eq!(parse(r#"{"phone":null}"#).unwrap().phone, Some(None));
        assert_eq!(
            parse(r#"{"phone":"+385"}"#).unwrap().phone,
            Some(Some("+385".to_string()))
        );
        assert_eq!(parse(r#"{"tracked":false}"#).unwrap().tracked, Some(false));
    }

    #[test]
    fn null_for_a_field_that_cant_be_cleared_is_an_error() {
        assert!(parse(r#"{"tracked":null}"#).is_err());
    }

    #[test]
    fn a_misspelled_field_is_an_error() {
        assert!(parse(r#"{"phnoe":"+385"}"#).is_err());
    }
}
