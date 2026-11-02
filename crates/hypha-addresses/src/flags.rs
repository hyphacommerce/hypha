//! What an address is, as far as its text tells: a PO box, a US military address, a
//! parcel locker, letters outside the Latin script. Checkout rules read these.

use std::sync::LazyLock;

use hypha_core::{Address, AddressField, AddressFlags};
use regex::Regex;

/// "PO Box" in the languages that have asked, each between non-letters so "Boxer Street"
/// isn't one; a line that starts "Box 12" is one too (Sweden, Finland). Added to as
/// countries ask.
const PO_BOX: &str = r"(?i)(?:^|[^\p{L}\p{N}])(?:p\.?\s*o\.?\s*box|p\.\s*o\.\s*b\.?|pob\s*\d|post\s+office\s+box|postfach|postbox|postboks|postbus|apartado(?:\s+de\s+correos|\s+postal)?|bo[iî]te\s+postale|b\.?\s?p\.?\s*\d|casella\s+postale|caixa\s+postal|skrytka\s+pocztowa|po[sš]tanski\s+pretinac|po[sš]tanski\s+fah)(?:[^\p{L}]|$)|^\s*box\s+\d";

/// DHL's lockers and counters: "Packstation 123", "Postfiliale 456".
const PARCEL_LOCKER: &str = r"(?i)(?:^|[^\p{L}])(?:packstation|postfiliale)\b\D*\d";

const NON_LATIN: &str = r"[\p{L}--\p{Latin}]";

static PATTERNS: LazyLock<Option<[Regex; 3]>> = LazyLock::new(|| {
    Some([
        Regex::new(PO_BOX).ok()?,
        Regex::new(PARCEL_LOCKER).ok()?,
        Regex::new(NON_LATIN).ok()?,
    ])
});

pub(crate) fn of(address: &Address) -> AddressFlags {
    let Some([po_box, parcel_locker, non_latin]) = PATTERNS.as_ref() else {
        return AddressFlags::default();
    };
    let parts = address.parts();
    let lines = [parts.line1.as_str(), parts.line2.as_deref().unwrap_or("")];
    let country = address.country().as_str();
    AddressFlags {
        po_box: lines.iter().any(|l| po_box.is_match(l)),
        military: country == "US"
            && (matches!(
                parts.city.trim().to_uppercase().as_str(),
                "APO" | "FPO" | "DPO"
            ) || matches!(parts.province.as_deref(), Some("AA" | "AE" | "AP"))),
        parcel_locker: country == "DE" && lines.iter().any(|l| parcel_locker.is_match(l)),
        non_latin: AddressField::ALL
            .iter()
            .filter_map(|f| f.of(parts))
            .any(|v| non_latin.is_match(v)),
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::AddressParts;

    use super::*;

    fn address(country: &str, line1: &str, line2: Option<&str>) -> Address {
        Address::new(AddressParts {
            first_name: None,
            last_name: "Horvat".to_string(),
            company: None,
            line1: line1.to_string(),
            line2: line2.map(str::to_string),
            city: "Somewhere".to_string(),
            province: None,
            postal_code: None,
            country: country.to_string(),
            phone: None,
        })
        .unwrap()
    }

    fn po_box(line1: &str) -> bool {
        of(&address("US", line1, None)).po_box
    }

    #[test]
    fn finds_po_boxes_in_several_spellings() {
        for line in [
            "PO Box 12",
            "P.O. Box 12",
            "p o box 12",
            "P.O.B. 77",
            "Post Office Box 9",
            "Postfach 10 20 30",
            "Apartado de correos 5",
            "Boîte postale 18",
            "Casella postale 4",
            "Postbus 100",
            "Skrytka pocztowa 3",
            "Poštanski pretinac 8",
            "Box 123",
        ] {
            assert!(po_box(line), "{line}");
        }
    }

    #[test]
    fn a_street_named_like_a_box_is_not_one() {
        assert!(!po_box("12 Boxer Street"));
        assert!(!po_box("3 Box Hill Road"));
        assert!(!po_box("7 Pobble Lane"));
    }

    #[test]
    fn reads_the_second_line_too() {
        assert!(of(&address("US", "1 Main St", Some("PO Box 4"))).po_box);
    }

    #[test]
    fn a_us_military_city_is_military() {
        let mut parts = address("US", "Unit 2050 Box 4190", None).parts().clone();
        parts.city = "APO".to_string();
        assert!(of(&Address::new(parts).unwrap()).military);
    }

    #[test]
    fn an_armed_forces_state_is_military() {
        let mut parts = address("US", "PSC 1234", None).parts().clone();
        parts.province = Some("AE".to_string());
        assert!(of(&Address::new(parts).unwrap()).military);
    }

    #[test]
    fn military_is_only_us() {
        let mut parts = address("CA", "1 Rue", None).parts().clone();
        parts.city = "APO".to_string();
        assert!(!of(&Address::new(parts).unwrap()).military);
    }

    #[test]
    fn finds_a_packstation_in_germany() {
        assert!(of(&address("DE", "Packstation 123", Some("12345678"))).parcel_locker);
        assert!(!of(&address("AT", "Packstation 123", None)).parcel_locker);
    }

    #[test]
    fn a_packstation_needs_its_number() {
        assert!(!of(&address("DE", "Packstationweg", None)).parcel_locker);
    }

    #[test]
    fn non_latin_letters_are_flagged() {
        assert!(of(&address("JP", "東京都千代田区1", None)).non_latin);
        assert!(of(&address("RS", "Кнез Михаилова 30", None)).non_latin);
    }

    #[test]
    fn accented_latin_letters_are_not() {
        assert!(!of(&address("HR", "Šoštarićeva 1", None)).non_latin);
        assert!(!of(&address("DE", "Torstraße 140", None)).non_latin);
    }
}
