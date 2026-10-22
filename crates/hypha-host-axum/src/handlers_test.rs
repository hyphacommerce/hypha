//! The handlers and payment providers the native host runs (core's from `hypha-http`,
//! joined with its spores' by `PgStorage`), checked against its storage.

use std::collections::HashSet;
use std::sync::Arc;

use hypha_core::{EventHandlers, Union};
use hypha_http::host::{HostMail, MailQueued};
use hypha_http::mail::{MailHost, MailParts};
use hypha_http::state::OrderTokens;
use hypha_payments::PaymentProviders;
use hypha_spore_core::{Built, SporeSet};

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

/// The stock build's spores, built on a test host.
fn built() -> Built<<Stock as Deployment>::Spores> {
    <<Stock as Deployment>::Spores as SporeSet>::build(
        &hypha_spore_core::env::testing::host(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn handler_names_are_unique_snake_case() {
    let handlers = Union((
        hypha_http::handlers::core_handlers(mail()),
        built().handlers,
    ));
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
    let keys = Union((
        hypha_http::payments::CORE_PROVIDERS,
        built().payment_providers,
    ))
    .keys();
    let unique: HashSet<_> = keys.iter().collect();
    assert_eq!(unique.len(), keys.len(), "{keys:?}");
}
