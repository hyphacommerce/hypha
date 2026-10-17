//! The handlers the native host runs (`handlers.rs`), checked against its storage.

use std::collections::HashSet;

use hypha_core::EventHandlers;

use crate::handlers::HANDLERS;

use crate::storage::PgStorage;

type Ports<'a> = <PgStorage as hypha_http::state::Storage>::Ports<'a>;

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
