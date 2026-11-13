//! The translation commands: who may read and write them, which languages they may be
//! in, and what a write stores, replaces and removes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]
use std::collections::BTreeMap;

use hypha_core::{
    Actor, AuthError, Ctx, EventHandler, Language, LanguageError, PageRequest, Scope, StoredEvent,
    Timestamp, UserId, Version, Versioned,
};
use hypha_testkit::InMemoryPorts;
use hypha_translations::{
    CommandError, FieldInput, ResourceKind, ResourceRef, Source, SourceField, StoreLanguages,
    Translation, TranslationCleanup, TranslationRepo, delete_translation, get_translations,
    list_translations, patch_translation, put_translation,
};
use pollster::block_on;
use serde_json::json;
use uuid::Uuid;

fn product(n: u128) -> ResourceRef {
    ResourceRef::new(ResourceKind::Product, Uuid::from_u128(n))
}

fn lang(tag: &str) -> Language {
    Language::new(tag).unwrap()
}

fn source() -> Source {
    Source::new(vec![
        SourceField::new("title", Some("Mug")),
        SourceField::new("description", Some("<p>A mug.</p>")),
        SourceField::new("name", Some("Mugs")),
    ])
}

fn fields(fields: &[(&str, &str)]) -> BTreeMap<String, FieldInput> {
    fields
        .iter()
        .map(|(key, value)| {
            (
                key.to_string(),
                FieldInput {
                    value: value.to_string(),
                    source_digest: None,
                },
            )
        })
        .collect()
}

fn with(scopes: &[Scope]) -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        scopes.to_vec(),
    )
}

/// Writes `resource`'s translation into `language` as the system, in a store whose
/// content is English with German, Austrian German and French beside it.
fn put(
    ports: &InMemoryPorts,
    resource: ResourceRef,
    language: &str,
    input: &[(&str, &str)],
) -> Result<Option<Translation>, CommandError> {
    put_as(&Ctx::system(), ports, resource, language, input)
}

fn put_as(
    ctx: &Ctx,
    ports: &InMemoryPorts,
    resource: ResourceRef,
    language: &str,
    input: &[(&str, &str)],
) -> Result<Option<Translation>, CommandError> {
    let others = [lang("de"), lang("de-AT"), lang("fr")];
    block_on(put_translation(
        ctx,
        ports,
        resource,
        language,
        StoreLanguages {
            default: &lang("en"),
            others: &others,
        },
        Some(&source()),
        fields(input),
    ))
}

#[test]
fn a_first_write_stores_the_translation() {
    let ports = InMemoryPorts::default();
    let written = put(&ports, product(1), "de", &[("title", "Tasse")])
        .unwrap()
        .unwrap();
    assert_eq!(written.version(), Version::new(1));
    assert_eq!(written.text("title"), Some("Tasse"));
    assert_eq!(
        block_on(get_translations(&Ctx::system(), &ports, product(1))),
        Ok(vec![written])
    );
}

#[test]
fn a_second_write_replaces_the_fields() {
    let ports = InMemoryPorts::default();
    put(
        &ports,
        product(1),
        "de",
        &[("title", "Tasse"), ("description", "<p>Eine Tasse.</p>")],
    )
    .unwrap();
    let written = put(&ports, product(1), "de", &[("title", "Becher")])
        .unwrap()
        .unwrap();
    assert_eq!(written.version(), Version::new(2));
    assert_eq!(written.text("title"), Some("Becher"));
    assert_eq!(written.text("description"), None);
}

fn patch(
    ports: &InMemoryPorts,
    resource: ResourceRef,
    language: &str,
    input: &[(&str, Option<&str>)],
) -> Result<Option<Translation>, CommandError> {
    let others = [lang("de"), lang("de-AT"), lang("fr")];
    let input = input
        .iter()
        .map(|(key, value)| {
            (
                key.to_string(),
                value.map(|value| FieldInput {
                    value: value.to_string(),
                    source_digest: None,
                }),
            )
        })
        .collect();
    block_on(patch_translation(
        &Ctx::system(),
        ports,
        resource,
        language,
        StoreLanguages {
            default: &lang("en"),
            others: &others,
        },
        Some(&source()),
        input,
    ))
}

#[test]
fn a_patch_changes_only_the_fields_it_names() {
    let ports = InMemoryPorts::default();
    put(
        &ports,
        product(1),
        "de",
        &[("title", "Tasse"), ("description", "<p>Eine Tasse.</p>")],
    )
    .unwrap();
    let written = patch(&ports, product(1), "de", &[("title", Some("Becher"))])
        .unwrap()
        .unwrap();
    assert_eq!(written.version(), Version::new(2));
    assert_eq!(written.text("title"), Some("Becher"));
    assert_eq!(written.text("description"), Some("<p>Eine Tasse.</p>"));
    let last = ports.outbox.events().pop().unwrap();
    assert_eq!(last.data["fields"], json!(["title"]));
}

#[test]
fn a_patch_creates_the_translation_and_deletes_it_when_none_is_left() {
    let ports = InMemoryPorts::default();
    let created = patch(&ports, product(1), "de", &[("title", Some("Tasse"))])
        .unwrap()
        .unwrap();
    assert_eq!(created.version(), Version::new(1));
    assert_eq!(
        patch(&ports, product(1), "de", &[("title", None)]),
        Ok(None)
    );
    assert_eq!(
        block_on(get_translations(&Ctx::system(), &ports, product(1))),
        Ok(Vec::new())
    );
    // Removing what isn't there stores nothing.
    assert_eq!(
        patch(&ports, product(1), "de", &[("title", None)]),
        Ok(None)
    );
    assert_eq!(ports.outbox.events().len(), 2);
}

#[test]
fn a_patch_takes_the_same_languages_as_a_put() {
    let ports = InMemoryPorts::default();
    assert_eq!(
        patch(&ports, product(1), "en", &[("title", Some("Mug"))]),
        Err(CommandError::DefaultLanguage(lang("en")))
    );
    assert_eq!(
        patch(&ports, product(1), "es", &[("title", Some("Taza"))]),
        Err(CommandError::LanguageNotListed(lang("es")))
    );
}

#[test]
fn writing_what_is_stored_writes_nothing() {
    let ports = InMemoryPorts::default();
    put(&ports, product(1), "de", &[("title", "Tasse")]).unwrap();
    let again = put(&ports, product(1), "de", &[("title", "Tasse")])
        .unwrap()
        .unwrap();
    assert_eq!(again.version(), Version::new(1));
    assert_eq!(ports.outbox.events().len(), 1);
}

#[test]
fn a_write_that_leaves_no_fields_deletes_the_translation() {
    let ports = InMemoryPorts::default();
    put(&ports, product(1), "de", &[("title", "Tasse")]).unwrap();
    assert_eq!(put(&ports, product(1), "de", &[("title", "")]), Ok(None));
    assert_eq!(
        block_on(get_translations(&Ctx::system(), &ports, product(1))),
        Ok(Vec::new())
    );
    let types: Vec<String> = ports
        .outbox
        .events()
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert_eq!(types, ["translation.updated", "translation.deleted"]);
}

#[test]
fn no_fields_for_a_resource_without_a_translation_stores_nothing() {
    let ports = InMemoryPorts::default();
    assert_eq!(put(&ports, product(1), "de", &[]), Ok(None));
    assert!(ports.outbox.events().is_empty());
}

#[test]
fn the_default_language_cant_be_translated_into() {
    let ports = InMemoryPorts::default();
    assert_eq!(
        put(&ports, product(1), "en", &[("title", "Mug")]),
        Err(CommandError::DefaultLanguage(lang("en")))
    );
}

#[test]
fn the_language_must_be_on_the_stores_list() {
    let ports = InMemoryPorts::default();
    assert_eq!(
        put(&ports, product(1), "es", &[("title", "Taza")]),
        Err(CommandError::LanguageNotListed(lang("es")))
    );
    // A regional wording is a language of its own: `de` on the list isn't `de-CH`.
    assert_eq!(
        put(&ports, product(1), "de-CH", &[("title", "Tasse")]),
        Err(CommandError::LanguageNotListed(lang("de-CH")))
    );
    assert!(put(&ports, product(1), "DE-at", &[("title", "Häferl")]).is_ok());
}

#[test]
fn a_tag_that_isnt_a_language_is_refused() {
    let ports = InMemoryPorts::default();
    assert_eq!(
        put(&ports, product(1), "german!", &[("title", "Tasse")]),
        Err(CommandError::Language(LanguageError::Invalid(
            "german!".to_string()
        )))
    );
}

#[test]
fn a_resource_the_host_didnt_find_is_not_found() {
    let ports = InMemoryPorts::default();
    let result = block_on(put_translation(
        &Ctx::system(),
        &ports,
        product(1),
        "de",
        StoreLanguages {
            default: &lang("en"),
            others: &[lang("de")],
        },
        None,
        fields(&[("title", "Tasse")]),
    ));
    assert_eq!(result, Err(CommandError::ResourceNotFound(product(1))));
}

#[test]
fn writing_takes_translations_write_and_the_resources_read_scope() {
    let ports = InMemoryPorts::default();
    let input = [("title", "Tasse")];
    assert_eq!(
        put_as(
            &with(&[Scope::ProductsRead, Scope::TranslationsRead]),
            &ports,
            product(1),
            "de",
            &input
        ),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::TranslationsWrite
        )))
    );
    assert_eq!(
        put_as(
            &with(&[Scope::TranslationsWrite]),
            &ports,
            product(1),
            "de",
            &input
        ),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsRead
        )))
    );
    assert!(
        put_as(
            &with(&[Scope::ProductsRead, Scope::TranslationsWrite]),
            &ports,
            product(1),
            "de",
            &input
        )
        .is_ok()
    );
}

#[test]
fn reading_takes_translations_read_and_the_resources_read_scope() {
    let ports = InMemoryPorts::default();
    let read = |ctx: &Ctx| block_on(get_translations(ctx, &ports, product(1))).map(|_| ());
    assert_eq!(
        read(&with(&[Scope::ProductsRead])),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::TranslationsRead
        )))
    );
    assert_eq!(
        read(&with(&[Scope::TranslationsRead])),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsRead
        )))
    );
    // A write scope implies its read scope.
    assert_eq!(
        read(&with(&[Scope::ProductsWrite, Scope::TranslationsWrite])),
        Ok(())
    );
}

#[test]
fn the_stores_own_text_takes_both_of_its_read_scopes() {
    let ports = InMemoryPorts::default();
    let store = ResourceRef::new(ResourceKind::Store, Uuid::from_u128(1));
    let read = |ctx: &Ctx| block_on(get_translations(ctx, &ports, store)).map(|_| ());
    assert_eq!(
        read(&with(&[Scope::TranslationsRead, Scope::StoresRead])),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::NotificationsRead
        )))
    );
    assert_eq!(
        read(&with(&[
            Scope::TranslationsRead,
            Scope::StoresRead,
            Scope::NotificationsRead
        ])),
        Ok(())
    );
}

#[test]
fn deleting_removes_one_language() {
    let ports = InMemoryPorts::default();
    put(&ports, product(1), "de", &[("title", "Tasse")]).unwrap();
    put(&ports, product(1), "fr", &[("title", "Tasse")]).unwrap();
    block_on(delete_translation(&Ctx::system(), &ports, product(1), "de")).unwrap();
    let left = block_on(get_translations(&Ctx::system(), &ports, product(1))).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].language(), &lang("fr"));
    assert_eq!(
        block_on(delete_translation(&Ctx::system(), &ports, product(1), "de")),
        Err(CommandError::NotFound {
            resource: product(1),
            language: lang("de")
        })
    );
}

#[test]
fn deleting_takes_translations_write() {
    let ports = InMemoryPorts::default();
    put(&ports, product(1), "de", &[("title", "Tasse")]).unwrap();
    assert_eq!(
        block_on(delete_translation(
            &with(&[Scope::ProductsRead, Scope::TranslationsRead]),
            &ports,
            product(1),
            "de"
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::TranslationsWrite
        )))
    );
}

#[test]
fn a_list_shows_only_the_kinds_the_caller_reads() {
    let ports = InMemoryPorts::default();
    let category = ResourceRef::new(ResourceKind::Category, Uuid::from_u128(2));
    let discount = ResourceRef::new(ResourceKind::Discount, Uuid::from_u128(3));
    put(&ports, product(1), "de", &[("title", "Tasse")]).unwrap();
    put(&ports, category, "de", &[("name", "Tassen")]).unwrap();
    put(&ports, discount, "fr", &[("title", "Soldes")]).unwrap();
    let kinds = |ctx: &Ctx, kind, language: Option<&str>| {
        block_on(list_translations(
            ctx,
            &ports,
            kind,
            language.map(lang),
            PageRequest::default(),
        ))
        .map(|page| {
            page.items
                .iter()
                .map(|t| t.resource().kind)
                .collect::<Vec<_>>()
        })
    };
    let catalogue = with(&[Scope::TranslationsRead, Scope::ProductsRead]);
    assert_eq!(
        kinds(&Ctx::system(), None, None),
        Ok(vec![
            ResourceKind::Category,
            ResourceKind::Discount,
            ResourceKind::Product
        ])
    );
    assert_eq!(
        kinds(&catalogue, None, None),
        Ok(vec![ResourceKind::Category, ResourceKind::Product])
    );
    assert_eq!(
        kinds(&Ctx::system(), None, Some("fr")),
        Ok(vec![ResourceKind::Discount])
    );
    // Asking for a kind by name takes its scope.
    assert_eq!(
        kinds(&catalogue, Some(ResourceKind::Discount), None),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::DiscountsRead
        )))
    );
    assert_eq!(
        kinds(&with(&[Scope::ProductsRead]), None, None),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::TranslationsRead
        )))
    );
}

fn event(event_type: &str, data: serde_json::Value) -> StoredEvent {
    StoredEvent {
        seq: 1,
        id: Uuid::from_u128(1),
        event_type: event_type.to_string(),
        version: 1,
        subject_kind: "product".to_string(),
        subject_id: Uuid::from_u128(1),
        subject_version: None,
        actor: Actor::System,
        occurred_at: Timestamp::from_unix_micros(0),
        data,
    }
}

#[test]
fn the_cleanup_removes_a_deleted_resources_translations() {
    let ports = InMemoryPorts::default();
    let category = ResourceRef::new(ResourceKind::Category, Uuid::from_u128(1));
    put(&ports, product(1), "de", &[("title", "Tasse")]).unwrap();
    put(&ports, product(1), "fr", &[("title", "Tasse")]).unwrap();
    put(&ports, product(2), "de", &[("title", "Teller")]).unwrap();
    put(&ports, category, "de", &[("name", "Tassen")]).unwrap();

    let deleted = event(
        "product.deleted",
        json!({"product": product(1).id_string(), "handle": "mug", "variants": []}),
    );
    let handler = TranslationCleanup;
    assert!(EventHandler::<InMemoryPorts>::wants(
        &handler,
        "product.deleted"
    ));
    assert!(!EventHandler::<InMemoryPorts>::wants(
        &handler,
        "product.updated"
    ));
    block_on(handler.handle(&ports, &deleted)).unwrap();
    // Again: nothing left to do.
    block_on(handler.handle(&ports, &deleted)).unwrap();

    assert_eq!(
        block_on(ports.translations.for_resource(product(1))),
        Ok(Vec::new())
    );
    // The category with the same ID and the other product keep theirs.
    assert_eq!(
        block_on(ports.translations.for_resource(category))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        block_on(ports.translations.for_resource(product(2)))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn the_cleanup_follows_every_kind_with_a_deleted_event() {
    let ports = InMemoryPorts::default();
    let cases = [
        (
            "category.deleted",
            ResourceKind::Category,
            "category",
            "name",
        ),
        (
            "custom_field_definition.deleted",
            ResourceKind::CustomFieldDefinition,
            "definition",
            "name",
        ),
        (
            "discount.deleted",
            ResourceKind::Discount,
            "discount",
            "title",
        ),
        (
            "checkout_rule.deleted",
            ResourceKind::CheckoutRule,
            "checkout_rule",
            "message",
        ),
        ("asset.deleted", ResourceKind::Asset, "asset", "alt"),
    ];
    for (event_type, kind, field, key) in cases {
        let resource = ResourceRef::new(kind, Uuid::from_u128(7));
        let source = Source::new(vec![SourceField::new(key, Some("Source"))]);
        block_on(put_translation(
            &Ctx::system(),
            &ports,
            resource,
            "de",
            StoreLanguages {
                default: &lang("en"),
                others: &[lang("de")],
            },
            Some(&source),
            fields(&[(key, "Übersetzung")]),
        ))
        .unwrap();
        let deleted = event(event_type, json!({ field: resource.id_string() }));
        block_on(TranslationCleanup.handle(&ports, &deleted)).unwrap();
        assert_eq!(
            block_on(ports.translations.for_resource(resource)),
            Ok(Vec::new()),
            "{event_type}"
        );
    }
}

#[test]
fn the_cleanup_fails_on_an_event_that_names_no_resource() {
    let ports = InMemoryPorts::default();
    let broken = event("product.deleted", json!({"handle": "mug"}));
    assert!(block_on(TranslationCleanup.handle(&ports, &broken)).is_err());
}
