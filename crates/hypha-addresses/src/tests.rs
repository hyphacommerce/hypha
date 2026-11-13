use hypha_core::{AddressError, AddressParts};

use super::*;

fn parts(country: &str) -> AddressParts {
    AddressParts {
        first_name: Some("Ana".to_string()),
        last_name: "Horvat".to_string(),
        company: None,
        line1: "1 Main Street".to_string(),
        line2: None,
        city: "Somewhere".to_string(),
        province: None,
        postal_code: None,
        country: country.to_string(),
        phone: None,
    }
}

fn us(province: Option<&str>, postal_code: Option<&str>) -> AddressParts {
    AddressParts {
        province: province.map(str::to_string),
        postal_code: postal_code.map(str::to_string),
        ..parts("US")
    }
}

fn problems(parts: AddressParts) -> Vec<(AddressField, AddressProblemCode)> {
    match Formats.validate(parts) {
        Ok(_) => Vec::new(),
        Err(e) => e.problems().iter().map(|p| (p.field, p.code)).collect(),
    }
}

fn checked(parts: AddressParts) -> CheckedAddress {
    Formats.validate(parts).unwrap()
}

fn code(country: &str) -> CountryCode {
    CountryCode::new(country).unwrap()
}

#[test]
fn every_country_example_matches_its_pattern() {
    for country in &countries::COUNTRIES {
        if let (Some(example), Some((pattern, exclude))) =
            (country.postal_example, patterns(country))
        {
            let example = normalise_postal_code(example);
            assert!(pattern.is_match(&example), "{} {example}", country.code);
            assert!(
                !exclude.is_some_and(|e| e.is_match(&example)),
                "{}",
                country.code
            );
        }
    }
}

#[test]
fn every_pattern_compiles() {
    for country in &countries::COUNTRIES {
        if country.postal_pattern.is_some() {
            assert!(patterns(country).is_some(), "{}", country.code);
        }
    }
}

#[test]
fn countries_are_sorted_for_lookup() {
    assert!(
        countries::COUNTRIES
            .windows(2)
            .all(|w| w[0].code < w[1].code)
    );
}

#[test]
fn a_valid_us_address_passes() {
    assert_eq!(problems(us(Some("CA"), Some("94043"))), []);
}

#[test]
fn the_region_becomes_its_code() {
    let address = checked(us(Some("california"), Some("94043"))).address;
    assert_eq!(address.parts().province.as_deref(), Some("CA"));
}

#[test]
fn a_region_code_is_matched_case_insensitively() {
    let address = checked(us(Some("ca"), Some("94043"))).address;
    assert_eq!(address.parts().province.as_deref(), Some("CA"));
}

#[test]
fn an_alternate_region_name_is_matched() {
    let address = checked(AddressParts {
        province: Some("Araba".to_string()),
        postal_code: Some("01001".to_string()),
        line1: "Calle Mayor 1".to_string(),
        ..parts("ES")
    })
    .address;
    assert_eq!(address.parts().province.as_deref(), Some("VI"));
}

#[test]
fn an_unknown_region_is_a_problem() {
    assert_eq!(
        problems(us(Some("Kalifornia"), Some("94043"))),
        [(AddressField::Province, AddressProblemCode::UnknownRegion)]
    );
}

#[test]
fn a_listed_region_is_required() {
    assert_eq!(
        problems(us(None, Some("94043"))),
        [(AddressField::Province, AddressProblemCode::Required)]
    );
}

#[test]
fn a_region_is_free_text_where_the_country_has_no_list() {
    let address = checked(AddressParts {
        province: Some("Grad Zagreb".to_string()),
        postal_code: Some("10000".to_string()),
        ..parts("HR")
    })
    .address;
    assert_eq!(address.parts().province.as_deref(), Some("Grad Zagreb"));
}

#[test]
fn a_region_the_form_hides_is_kept_unchecked() {
    let address = checked(AddressParts {
        province: Some("Somewhere else".to_string()),
        postal_code: Some("E2 7RG".to_string()),
        ..parts("GB")
    })
    .address;
    assert_eq!(address.parts().province.as_deref(), Some("Somewhere else"));
}

#[test]
fn a_required_postal_code_is_refused_empty() {
    assert_eq!(
        problems(parts("HR")),
        [(AddressField::PostalCode, AddressProblemCode::Required)]
    );
}

#[test]
fn a_postal_code_in_the_wrong_shape_is_a_problem() {
    assert_eq!(
        problems(us(Some("CA"), Some("1"))),
        [(AddressField::PostalCode, AddressProblemCode::InvalidFormat)]
    );
}

#[test]
fn the_shape_problem_names_the_example() {
    let Err(AddressError::Invalid(found)) = Formats.validate(us(Some("CA"), Some("1"))) else {
        panic!("expected problems");
    };
    assert_eq!(found[0].message, "Enter a valid ZIP code, like 90210");
}

#[test]
fn the_shape_problem_gives_the_example_as_a_param() {
    let Err(AddressError::Invalid(found)) = Formats.validate(us(Some("CA"), Some("1"))) else {
        panic!("expected problems");
    };
    assert_eq!(
        found[0].params,
        MessageParams {
            example: Some("90210".to_string()),
            ..MessageParams::field(AddressField::PostalCode)
        }
    );
}

#[test]
fn a_mismatch_names_the_region_as_params() {
    let Err(AddressError::Invalid(found)) = Formats.validate(us(Some("NY"), Some("94043"))) else {
        panic!("expected problems");
    };
    assert_eq!(
        (
            found[0].params.region.as_deref(),
            found[0].params.region_name.as_deref()
        ),
        (Some("NY"), Some("New York"))
    );
}

#[test]
fn a_missing_region_names_its_field_as_params() {
    let Err(AddressError::Invalid(found)) = Formats.validate(us(None, Some("94043"))) else {
        panic!("expected problems");
    };
    assert_eq!(
        found[0].params,
        MessageParams::field(AddressField::Province)
    );
}

#[test]
fn a_postal_code_is_upper_cased_with_spaces_collapsed() {
    let address = checked(AddressParts {
        postal_code: Some(" e2   7rg ".to_string()),
        ..parts("GB")
    })
    .address;
    assert_eq!(address.parts().postal_code.as_deref(), Some("E2 7RG"));
}

#[test]
fn a_postal_code_of_another_region_is_a_mismatch() {
    assert_eq!(
        problems(us(Some("NY"), Some("94043"))),
        [(AddressField::PostalCode, AddressProblemCode::RegionMismatch)]
    );
}

#[test]
fn a_country_prefix_on_the_postal_code_still_places_it() {
    assert_eq!(
        problems(AddressParts {
            province: Some("M".to_string()),
            postal_code: Some("ES-08001".to_string()),
            line1: "Calle Mayor 1".to_string(),
            ..parts("ES")
        }),
        [(AddressField::PostalCode, AddressProblemCode::RegionMismatch)]
    );
}

#[test]
fn a_negative_lookahead_still_refuses_its_codes() {
    // Denmark's pattern refuses 38xx and 39xx (the Faroes and Greenland).
    assert_eq!(
        problems(AddressParts {
            postal_code: Some("3800".to_string()),
            ..parts("DK")
        }),
        [(AddressField::PostalCode, AddressProblemCode::InvalidFormat)]
    );
    assert_eq!(
        problems(AddressParts {
            postal_code: Some("2100".to_string()),
            ..parts("DK")
        }),
        []
    );
}

#[test]
fn a_postal_code_is_dropped_where_the_country_has_none() {
    let address = checked(AddressParts {
        postal_code: Some("999077".to_string()),
        province: Some("Kowloon".to_string()),
        ..parts("HK")
    })
    .address;
    assert_eq!(address.parts().postal_code, None);
}

#[test]
fn a_recommended_postal_code_may_be_left_out() {
    assert_eq!(Form::of(code("TR")).postal_code(), PostalCode::Recommended);
    assert_eq!(problems(parts("TR")), []);
}

#[test]
fn structural_and_format_problems_come_together() {
    assert_eq!(
        problems(AddressParts {
            city: String::new(),
            ..us(Some("Kalifornia"), Some("1"))
        }),
        [
            (AddressField::City, AddressProblemCode::Required),
            (AddressField::Province, AddressProblemCode::UnknownRegion),
            (AddressField::PostalCode, AddressProblemCode::InvalidFormat),
        ]
    );
}

#[test]
fn a_missing_house_number_warns() {
    let warned = checked(AddressParts {
        line1: "Prinsengracht".to_string(),
        postal_code: Some("1016 GV".to_string()),
        ..parts("NL")
    });
    assert_eq!(
        warned
            .warnings
            .iter()
            .map(|w| (w.field, w.code))
            .collect::<Vec<_>>(),
        [(AddressField::Line1, AddressWarningCode::HouseNumberMissing)]
    );
}

#[test]
fn a_house_number_is_only_expected_where_the_country_does() {
    let address = checked(AddressParts {
        line1: "Ilica".to_string(),
        postal_code: Some("10000".to_string()),
        ..parts("HR")
    });
    assert!(address.warnings.is_empty());
}

#[test]
fn check_answers_the_flags() {
    let found = checked(AddressParts {
        line1: "PO Box 12".to_string(),
        ..us(Some("CA"), Some("94043"))
    });
    assert!(found.flags.po_box);
}

#[test]
fn region_name_reads_a_listed_code() {
    assert_eq!(region_name(code("US"), "CA"), Some("California"));
    assert_eq!(region_name(code("HR"), "Grad Zagreb"), None);
}

#[test]
fn the_us_form_follows_its_country() {
    let form = Form::of(code("US"));
    assert_eq!(
        form.rows()[4],
        [
            AddressField::City,
            AddressField::Province,
            AddressField::PostalCode
        ]
    );
    assert_eq!(
        form.required(),
        [
            AddressField::LastName,
            AddressField::Line1,
            AddressField::City,
            AddressField::Province,
            AddressField::PostalCode,
        ]
    );
    let labels = form.labels();
    assert!(labels.contains(&(AddressField::Province, "State")));
    assert!(labels.contains(&(AddressField::PostalCode, "ZIP code")));
    assert_eq!(form.postal_code_example(), Some("90210"));
    assert!(
        form.regions()
            .any(|r| r.code == "AL" && r.name == "Alabama")
    );
}

#[test]
fn a_country_without_regions_lists_none() {
    assert_eq!(Form::of(code("HR")).regions().count(), 0);
}

#[test]
fn a_country_worldwide_lacks_gets_the_common_form() {
    // Antarctica: in ISO 3166, not in worldwide's list.
    let antarctica = code("AQ");
    if index(antarctica).is_none() {
        assert_eq!(Form::of(antarctica).rows(), OTHER.rows);
        assert!(problems(parts("AQ")).is_empty());
    }
}

#[test]
fn a_forms_labels_have_keys() {
    let keys = Form::of(CountryCode::new("US").unwrap()).label_keys();
    let key = |field| keys.iter().find(|(f, _)| *f == field).and_then(|(_, k)| *k);
    assert_eq!(
        (
            key(AddressField::Province),
            key(AddressField::PostalCode),
            key(AddressField::Line1)
        ),
        (
            Some(LabelKey::State),
            Some(LabelKey::ZipCode),
            Some(LabelKey::Address1)
        )
    );
}
