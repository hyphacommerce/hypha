//! Each country's address format, from a snapshot of Shopify's `worldwide` data
//! (`generated/`, made by `scripts/address-data.mjs`): the form's fields and labels,
//! which are required, postal code patterns, regions with their codes, alternate names
//! and postal code prefixes. `Formats` implements core's `AddressFormats`, which every
//! write of an address goes through.

#[path = "generated/countries.rs"]
mod countries;
mod data;
mod flags;
mod labels;

use std::sync::OnceLock;

use hypha_core::address::{in_sentence, with_article};
use hypha_core::{
    Address, AddressField, AddressFormats, AddressParts, AddressProblem, AddressProblemCode,
    AddressWarning, AddressWarningCode, CheckedAddress, CountryCode, MessageParams,
};
use regex::Regex;

pub use data::PostalCode;
use data::{Country, OTHER, Region};
pub use labels::LabelKey;

/// The address formats of every country, compiled in.
#[derive(Debug, Clone, Copy, Default)]
pub struct Formats;

impl AddressFormats for Formats {
    fn check(&self, address: &Address) -> CheckedAddress {
        let country = country(address.country());
        let mut parts = address.parts().clone();
        let mut problems = Vec::new();
        let mut warnings = Vec::new();

        let region = normalise_region(country, &mut parts, &mut problems);
        check_postal_code(country, region, &mut parts, &mut problems);

        if country.house_number_required && !parts.line1.chars().any(|c| c.is_ascii_digit()) {
            warnings.push(AddressWarning {
                field: AddressField::Line1,
                code: AddressWarningCode::HouseNumberMissing,
                message: "Add a house number".to_string(),
            });
        }

        // Normalising only changes the region and the postal code, which `Address::new`
        // already took: it can't refuse them now, but keep what was given if it did.
        let normalised = Address::new(parts).unwrap_or_else(|_| address.clone());
        let flags = flags::of(&normalised);
        CheckedAddress {
            address: normalised,
            problems,
            warnings,
            flags,
        }
    }

    fn region_name(&self, country: CountryCode, code: &str) -> Option<&str> {
        region_name(country, code)
    }
}

/// The name of `country`'s region `code`, where the country lists its regions.
pub fn region_name(country: CountryCode, code: &str) -> Option<&'static str> {
    self::country(country)
        .regions
        .iter()
        .find(|r| r.code == code)
        .map(|r| r.name)
}

/// What an address form for a country shows: the fields in the country's order, which
/// are required, their labels and the regions to choose from.
#[derive(Clone, Copy)]
pub struct Form {
    country: CountryCode,
    format: &'static Country,
}

/// A region to choose from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionOption {
    pub code: &'static str,
    pub name: &'static str,
}

impl std::fmt::Debug for Form {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Form")
            .field("country", &self.country)
            .finish()
    }
}

impl Form {
    pub fn of(country: CountryCode) -> Self {
        Self {
            country,
            format: self::country(country),
        }
    }

    pub fn country(&self) -> CountryCode {
        self.country
    }

    /// The fields row by row, the country itself apart.
    pub fn rows(&self) -> &'static [&'static [AddressField]] {
        self.format.rows
    }

    /// Every field the form shows that a write refuses empty.
    pub fn required(&self) -> Vec<AddressField> {
        self.rows()
            .iter()
            .flat_map(|row| row.iter().copied())
            .filter(|field| self.is_required(*field))
            .collect()
    }

    fn is_required(&self, field: AddressField) -> bool {
        match field {
            AddressField::LastName | AddressField::Line1 | AddressField::City => true,
            AddressField::Province => self.format.region_required,
            AddressField::PostalCode => self.format.postal_code == PostalCode::Required,
            _ => false,
        }
    }

    pub fn postal_code(&self) -> PostalCode {
        self.format.postal_code
    }

    /// The label of each field the form shows.
    pub fn labels(&self) -> Vec<(AddressField, &'static str)> {
        self.rows()
            .iter()
            .flat_map(|row| row.iter().copied())
            .map(|field| (field, label(self.format, field)))
            .collect()
    }

    /// The key of each field's label, as `labels` lists them: what a client translates
    /// the label by. `None` for a label without one.
    pub fn label_keys(&self) -> Vec<(AddressField, Option<LabelKey>)> {
        self.labels()
            .into_iter()
            .map(|(field, label)| (field, LabelKey::of(label)))
            .collect()
    }

    pub fn postal_code_example(&self) -> Option<&'static str> {
        self.format.postal_example
    }

    /// The regions to choose from; empty where the region is free text or not asked for.
    pub fn regions(&self) -> impl Iterator<Item = RegionOption> {
        self.format.regions.iter().map(|r| RegionOption {
            code: r.code,
            name: r.name,
        })
    }

    /// Whether the house number is expected in the first line.
    pub fn house_number_required(&self) -> bool {
        self.format.house_number_required
    }
}

fn country(code: CountryCode) -> &'static Country {
    index(code).map_or(&OTHER, |i| &countries::COUNTRIES[i])
}

fn index(code: CountryCode) -> Option<usize> {
    countries::COUNTRIES
        .binary_search_by(|c| c.code.cmp(code.as_str()))
        .ok()
}

fn label(country: &Country, field: AddressField) -> &'static str {
    country
        .labels
        .iter()
        .find(|(f, _)| *f == field)
        .map_or_else(|| field.label(), |(_, label)| label)
}

/// Turns the region into its code where the country lists regions, and reports one
/// that isn't there (or is missing where required). Returns the region found.
fn normalise_region(
    country: &'static Country,
    parts: &mut AddressParts,
    problems: &mut Vec<AddressProblem>,
) -> Option<&'static Region> {
    let label = label(country, AddressField::Province);
    let Some(given) = parts.province.as_deref() else {
        if country.region_required {
            problems.push(AddressProblem {
                field: AddressField::Province,
                code: AddressProblemCode::Required,
                message: format!("Select {}", with_article(label)),
                params: MessageParams::field(AddressField::Province),
            });
        }
        return None;
    };
    if country.regions.is_empty() {
        return None;
    }
    let wanted = given.to_lowercase();
    let found = country.regions.iter().find(|r| {
        r.code.to_lowercase() == wanted
            || r.name.to_lowercase() == wanted
            || r.alternates
                .split('|')
                .any(|a| !a.is_empty() && a.to_lowercase() == wanted)
    });
    match found {
        Some(region) => {
            parts.province = Some(region.code.to_string());
            Some(region)
        }
        None => {
            problems.push(AddressProblem {
                field: AddressField::Province,
                code: AddressProblemCode::UnknownRegion,
                message: format!("Select a valid {}", in_sentence(label)),
                params: MessageParams::field(AddressField::Province),
            });
            None
        }
    }
}

/// Postal codes are compared upper-cased with their spaces collapsed.
fn normalise_postal_code(code: &str) -> String {
    code.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

fn check_postal_code(
    country: &'static Country,
    region: Option<&'static Region>,
    parts: &mut AddressParts,
    problems: &mut Vec<AddressProblem>,
) {
    let label = label(country, AddressField::PostalCode);
    if country.postal_code == PostalCode::None {
        parts.postal_code = None;
        return;
    }
    let Some(given) = parts.postal_code.as_deref() else {
        if country.postal_code == PostalCode::Required {
            problems.push(AddressProblem {
                field: AddressField::PostalCode,
                code: AddressProblemCode::Required,
                message: format!("Enter {}", with_article(label)),
                params: MessageParams::field(AddressField::PostalCode),
            });
        }
        return;
    };
    let code = normalise_postal_code(given);
    if let Some((pattern, exclude)) = patterns(country)
        && (!pattern.is_match(&code) || exclude.is_some_and(|e| e.is_match(&code)))
    {
        let message = match country.postal_example {
            Some(example) => format!("Enter a valid {}, like {example}", in_sentence(label)),
            None => format!("Enter a valid {}", in_sentence(label)),
        };
        problems.push(AddressProblem {
            field: AddressField::PostalCode,
            code: AddressProblemCode::InvalidFormat,
            message,
            params: MessageParams {
                example: country.postal_example.map(str::to_string),
                ..MessageParams::field(AddressField::PostalCode)
            },
        });
    } else if let Some(region) = region
        && placed(country, &code)
        && !in_region(region, &code)
    {
        problems.push(AddressProblem {
            field: AddressField::PostalCode,
            code: AddressProblemCode::RegionMismatch,
            message: format!("This {} isn't in {}", in_sentence(label), region.name),
            params: MessageParams {
                region: Some(region.code.to_string()),
                region_name: Some(region.name.to_string()),
                ..MessageParams::field(AddressField::PostalCode)
            },
        });
    }
    parts.postal_code = Some(code);
}

/// The ways a postal code is compared on prefixes: without spaces and hyphens, and
/// without a leading country prefix (`ES-28039`).
fn compact_forms(code: &str) -> [String; 2] {
    let compact: String = code.chars().filter(|c| *c != ' ' && *c != '-').collect();
    let bare = compact
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .to_string();
    [compact, bare]
}

fn in_region(region: &Region, code: &str) -> bool {
    let forms = compact_forms(code);
    region
        .prefixes
        .split(' ')
        .filter(|p| !p.is_empty())
        .any(|p| forms.iter().any(|f| f.starts_with(p)))
}

/// Whether some region's prefixes take `code`: where the data doesn't place a code, a
/// region can't be said not to match it.
fn placed(country: &Country, code: &str) -> bool {
    country.regions.iter().any(|r| in_region(r, code))
}

type Patterns = Option<(Regex, Option<Regex>)>;

static PATTERNS: [OnceLock<Patterns>; countries::COUNT] =
    [const { OnceLock::new() }; countries::COUNT];

/// The country's compiled postal code patterns, compiled on first use: a cold Worker
/// only pays for the countries it sees.
fn patterns(country: &'static Country) -> Option<(&'static Regex, Option<&'static Regex>)> {
    let i = countries::COUNTRIES
        .binary_search_by(|c| c.code.cmp(country.code))
        .ok()?;
    let compiled = PATTERNS[i].get_or_init(|| {
        // The generator checked every pattern; one that doesn't compile checks nothing.
        let pattern = Regex::new(country.postal_pattern?).ok()?;
        let exclude = country.postal_exclude.and_then(|e| Regex::new(e).ok());
        Some((pattern, exclude))
    });
    compiled.as_ref().map(|(p, e)| (p, e.as_ref()))
}

#[cfg(test)]
mod tests;
