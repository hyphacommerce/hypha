//! The handlers the native host runs (`handlers.rs`), checked against its storage.

use std::collections::HashSet;

use hypha_core::EventHandlers;

use std::sync::Arc;

use hypha_http::host::HostMail;
use hypha_http::mail::{MailHost, MailParts};
use hypha_http::state::OrderTokens;

use crate::storage::PgStorage;

type Ports<'a> = <PgStorage as hypha_http::state::Storage>::Ports<'a>;

#[test]
fn handler_names_are_unique_snake_case() {
    let mail = MailHost::new(
        MailParts {
            sender: HostMail::log(),
            from: hypha_core::Email::new("shop@example.com").unwrap(),
            store: hypha_core::StoreId::from_uuid(uuid::Uuid::nil()),
            admin_url: None,
            order_tokens: OrderTokens::new([7; 32]),
            media: None,
        },
        &crate::platform::runtime(),
    );
    let handlers = crate::handlers::handlers(Arc::new(mail));
    let names = EventHandlers::<Ports<'_>>::names(&handlers);
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
