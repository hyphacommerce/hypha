//! The event handlers this host runs (design note 0019, section 5): the composition
//! root for in-process destinations. A crate or spore with a handler adds it to the
//! tuple; the dispatcher runs whatever is here and knows none of them.

use hypha_dispatch::VariantCleanup;

/// Every handler, in no particular order (names must be unique; a test checks).
pub(crate) type Handlers = (VariantCleanup,);

pub(crate) const HANDLERS: Handlers = (VariantCleanup,);

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use hypha_core::EventHandlers;

    use super::*;
    use crate::state::PgStorage;

    type Ports<'a> = <PgStorage as crate::state::Storage>::Ports<'a>;

    #[test]
    fn handler_names_are_unique_snake_case() {
        let names = EventHandlers::<Ports<'_>>::names(&HANDLERS);
        let unique: HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "{names:?}");
        for name in names {
            assert!(
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{name:?}"
            );
        }
    }
}
