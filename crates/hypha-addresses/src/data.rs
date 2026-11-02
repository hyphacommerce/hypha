//! The shape of the generated tables (`generated/countries.rs`).

pub(crate) use hypha_core::AddressField as Field;

/// One country's address format.
pub(crate) struct Country {
    pub(crate) code: &'static str,
    /// The form's fields, row by row, in the country's order (the country itself apart).
    pub(crate) rows: &'static [&'static [Field]],
    pub(crate) postal_code: PostalCode,
    /// For the `regex` crate, anchored; matched against the normalised code.
    pub(crate) postal_pattern: Option<&'static str>,
    /// What a code must not start with, where the source pattern had a lookahead.
    pub(crate) postal_exclude: Option<&'static str>,
    pub(crate) postal_example: Option<&'static str>,
    /// Listed only where the form shows the region.
    pub(crate) regions: &'static [Region],
    pub(crate) region_required: bool,
    pub(crate) house_number_required: bool,
    /// The labels the country names its own way; the rest are `Field::label`'s.
    pub(crate) labels: &'static [(Field, &'static str)],
}

/// How a country uses postal codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostalCode {
    /// None: the form has no postal code, and one sent is dropped.
    None,
    Required,
    /// Asked for but optional.
    Recommended,
    Optional,
}

pub(crate) struct Region {
    pub(crate) code: &'static str,
    pub(crate) name: &'static str,
    /// Other names and codes, `|`-separated.
    pub(crate) alternates: &'static str,
    /// The postal codes' starts, space-separated, compared without spaces or hyphens.
    pub(crate) prefixes: &'static str,
}

/// A country worldwide doesn't list: the common form, nothing checked but the structure.
pub(crate) static OTHER: Country = Country {
    code: "",
    rows: &[
        &[Field::FirstName, Field::LastName],
        &[Field::Company],
        &[Field::Line1],
        &[Field::Line2],
        &[Field::City, Field::Province, Field::PostalCode],
        &[Field::Phone],
    ],
    postal_code: PostalCode::Optional,
    postal_pattern: None,
    postal_exclude: None,
    postal_example: None,
    regions: &[],
    region_required: false,
    house_number_required: false,
    labels: &[],
};
