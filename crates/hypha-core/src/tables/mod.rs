//! Reference tables compiled into core (design note 0021): plain sorted arrays, so they
//! cost no I/O, build for wasm and are searched with `binary_search`. Regenerate them
//! with `node scripts/reference-tables.mjs`; never edit the generated files by hand.

mod countries;
mod currencies;
mod timezones;

pub(crate) use countries::COUNTRIES;
pub(crate) use currencies::CURRENCIES;
pub(crate) use timezones::TIMEZONES;

#[cfg(test)]
mod tests {
    use super::*;

    /// `binary_search` silently misses entries in an unsorted table.
    fn assert_sorted_and_unique<'a>(codes: impl Iterator<Item = &'a str>) {
        let codes: Vec<&str> = codes.collect();
        for pair in codes.windows(2) {
            assert!(pair[0] < pair[1], "{} before {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn tables_are_sorted_without_duplicates() {
        assert_sorted_and_unique(CURRENCIES.iter().map(|c| c.0));
        assert_sorted_and_unique(COUNTRIES.iter().map(|c| c.0));
        assert_sorted_and_unique(TIMEZONES.iter().copied());
    }

    #[test]
    fn codes_have_their_iso_shape() {
        for (code, units, name) in CURRENCIES {
            assert!(code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()));
            assert!(units <= 3, "{code}");
            assert!(!name.is_empty(), "{code}");
        }
        for (code, name) in COUNTRIES {
            assert!(code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase()));
            assert!(!name.is_empty(), "{code}");
        }
    }
}
