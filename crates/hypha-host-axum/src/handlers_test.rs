//! The handlers and payment providers the native host runs (`handlers.rs`,
//! `payments.rs`, joined with its spores' by `PgStorage`), checked against its storage.

use std::collections::HashSet;
use std::sync::Arc;

use hypha_core::{EventHandlers, Union};
use hypha_http::host::{HostMail, MailQueued};
use hypha_http::mail::{MailHost, MailParts};
use hypha_http::state::OrderTokens;
use hypha_payments::PaymentProviders;
use hypha_spore_core::SporeSet;

use crate::{Deployment, PgPorts, Stock};

fn mail() -> Arc<MailHost> {
    Arc::new(MailHost::new(
        MailParts {
            sender: HostMail::log(),
            from: hypha_core::Email::new("shop@example.com").unwrap(),
            store: hypha_core::StoreId::from_uuid(uuid::Uuid::nil()),
            admin_url: None,
            order_tokens: OrderTokens::new([7; 32]),
            media: None,
            queued: MailQueued::none(),
        },
        &crate::platform::runtime(),
    ))
}

#[test]
fn handler_names_are_unique_snake_case() {
    let (spores, _) = <<Stock as Deployment>::Spores as SporeSet>::build();
    let handlers = Union((crate::handlers::core_handlers(mail()), spores));
    let names = EventHandlers::<PgPorts>::names(&handlers);
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

#[test]
fn provider_keys_are_unique() {
    let (_, spores) = <<Stock as Deployment>::Spores as SporeSet>::build();
    let keys = Union((crate::payments::CORE_PROVIDERS, spores)).keys();
    let unique: HashSet<_> = keys.iter().collect();
    assert_eq!(unique.len(), keys.len(), "{keys:?}");
}
