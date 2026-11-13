//! What every translations store must do: one translation per resource and language,
//! read for many resources at once, listed in key order, removed with their resource or
//! their language.

use std::collections::BTreeMap;

use hypha_core::{
    HasOutbox, Language, OutboxRepo, PageLimit, PageRequest, RepoError, Stamped, Version, Versioned,
};
use hypha_translations::{
    FieldInput, HasTranslations, Overlay, ResourceKind, ResourceRef, Source, SourceField,
    Translation, TranslationFilter, TranslationKey, TranslationRepo, keys,
};
use uuid::Uuid;

use super::scenario;

fn resource(kind: ResourceKind, n: u128) -> ResourceRef {
    ResourceRef::new(kind, Uuid::from_u128(n))
}

fn product(n: u128) -> ResourceRef {
    resource(ResourceKind::Product, n)
}

fn lang(tag: &str) -> Language {
    Language::new(tag).unwrap()
}

fn source() -> Source {
    Source::new(vec![
        SourceField::new("title", Some("Mug")),
        SourceField::new("description", Some("<p>A mug.</p>")),
        SourceField::new("options.opt_1.name", Some("Colour")),
        SourceField::new("name", Some("Mugs")),
        SourceField::new("alt", Some("A mug")),
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

/// A translation of `resource` into `language` with one field, `value`.
fn translation(resource: ResourceRef, language: &str, value: &str) -> Translation {
    let key = match resource.kind {
        ResourceKind::Category => "name",
        ResourceKind::Asset => "alt",
        _ => "title",
    };
    let mut t = Translation::new(resource, lang(language));
    t.replace(&source(), fields(&[(key, value)])).unwrap();
    t
}

async fn stored<S: HasTranslations>(ports: &S, resource: ResourceRef, language: &str, value: &str) {
    let mut t = translation(resource, language, value);
    ports.translations().insert(&mut t).await.unwrap();
}

fn keys_of(translations: &[Translation]) -> Vec<(ResourceRef, String)> {
    translations
        .iter()
        .map(|t| (t.resource(), t.language().to_string()))
        .collect()
}

/// Runs every `TranslationRepo` scenario, each on fresh ports from `make`.
pub async fn translation_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasTranslations + HasOutbox,
{
    scenario("translations: every field round trips, with its event");
    round_trip(&make().await).await;
    scenario("translations: one per resource and language");
    one_per_resource_and_language(&make().await).await;
    scenario("translations: update bumps the version, stale and missing are refused");
    versions(&make().await).await;
    scenario("translations: delete checks the version and writes its event");
    deletes(&make().await).await;
    scenario("translations: a resource's translations, by language");
    for_resource(&make().await).await;
    scenario("translations: many resources in the languages asked for, in one read");
    loads_many(&make().await).await;
    scenario("translations: the overlay falls back to the language without its region");
    overlay_falls_back(&make().await).await;
    scenario("translations: listed in key order, by kind and language, paged");
    lists(&make().await).await;
    scenario("translations: any_in says whether there's one into the languages named");
    any_in(&make().await).await;
    scenario("translations: a resource's go together, without events");
    deletes_resources(&make().await).await;
    scenario("translations: languages off the list go, without events");
    retains_languages(&make().await).await;
}

async fn round_trip<S: HasTranslations + HasOutbox>(ports: &S) {
    let repo = ports.translations();
    let de = lang("de");
    assert_eq!(repo.get(product(1), &de).await, Ok(None));

    let mut t = Translation::new(product(1), de.clone());
    t.replace(
        &source(),
        fields(&[
            ("title", "Tasse"),
            ("description", "<p>Eine Tasse für „Kaffee“.</p>"),
            ("options.opt_1.name", "Farbe"),
        ]),
    )
    .unwrap();
    repo.insert(&mut t).await.unwrap();
    assert_eq!(t.version(), Version::new(1));
    assert!(t.stamps().is_some());
    assert_eq!(repo.get(product(1), &de).await, Ok(Some(t.clone())));
    // Another resource with the same ID, and another language, are other translations.
    assert_eq!(
        repo.get(resource(ResourceKind::Category, 1), &de).await,
        Ok(None)
    );
    assert_eq!(repo.get(product(1), &lang("de-AT")).await, Ok(None));

    let events = ports.outbox().events_after(0, 100).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "translation.updated");
    assert_eq!(events[0].subject_kind, "translation");
    assert_eq!(events[0].subject_id, product(1).id);
    // The subject is the resource, and its languages each count their own versions.
    assert_eq!(events[0].subject_version, None);
    assert_eq!(
        events[0].data["fields"],
        serde_json::json!(["description", "options.opt_1.name", "title"])
    );
}

async fn one_per_resource_and_language<S: HasTranslations>(ports: &S) {
    let repo = ports.translations();
    stored(ports, product(1), "de", "Tasse").await;
    let mut again = translation(product(1), "de", "Becher");
    assert_eq!(
        repo.insert(&mut again).await,
        Err(RepoError::Duplicate(keys::TRANSLATION))
    );
    assert_eq!(
        repo.get(product(1), &lang("de"))
            .await
            .unwrap()
            .unwrap()
            .text("title"),
        Some("Tasse")
    );
    // The same language of another resource, and another language of this one, fit.
    stored(ports, product(2), "de", "Teller").await;
    stored(ports, product(1), "fr", "Tasse").await;
    stored(ports, resource(ResourceKind::Category, 1), "de", "Tassen").await;
}

async fn versions<S: HasTranslations + HasOutbox>(ports: &S) {
    let repo = ports.translations();
    let mut t = translation(product(1), "de", "Tasse");
    repo.insert(&mut t).await.unwrap();
    let created = t.stamps().unwrap();
    let mut stale = t.clone();

    t.replace(&source(), fields(&[("title", "Becher")]))
        .unwrap();
    repo.update(&mut t).await.unwrap();
    assert_eq!(t.version(), Version::new(2));
    assert_eq!(t.stamps().unwrap().created_at, created.created_at);
    assert_eq!(repo.get(product(1), &lang("de")).await, Ok(Some(t.clone())));

    stale
        .replace(&source(), fields(&[("title", "Krug")]))
        .unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(product(1), &lang("de")).await, Ok(Some(t)));

    let mut missing = translation(product(2), "de", "Teller");
    missing.set_version(Version::new(1));
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );

    let events = ports.outbox().events_after(0, 100).await.unwrap();
    let versions: Vec<_> = events.iter().map(|e| e.subject_version).collect();
    assert_eq!(versions, [None, None]);
}

async fn deletes<S: HasTranslations + HasOutbox>(ports: &S) {
    let repo = ports.translations();
    let de = lang("de");
    let mut t = translation(product(1), "de", "Tasse");
    repo.insert(&mut t).await.unwrap();
    stored(ports, product(1), "fr", "Tasse").await;
    let mut stale = t.clone();
    t.replace(&source(), fields(&[("title", "Becher")]))
        .unwrap();
    repo.update(&mut t).await.unwrap();

    stale.mark_deleted();
    assert_eq!(repo.delete(&mut stale).await, Err(RepoError::StaleVersion));
    assert!(repo.get(product(1), &de).await.unwrap().is_some());

    t.mark_deleted();
    repo.delete(&mut t).await.unwrap();
    assert_eq!(repo.get(product(1), &de).await, Ok(None));
    // The other language stays.
    assert!(repo.get(product(1), &lang("fr")).await.unwrap().is_some());
    let events = ports.outbox().events_after(0, 100).await.unwrap();
    let last = events.last().unwrap();
    assert_eq!(last.event_type, "translation.deleted");
    assert_eq!(last.subject_id, product(1).id);
    assert_eq!(last.data["language"], "de");

    let mut gone = translation(product(1), "de", "Tasse");
    gone.set_version(Version::new(2));
    gone.mark_deleted();
    assert_eq!(repo.delete(&mut gone).await, Err(RepoError::StaleVersion));
}

async fn for_resource<S: HasTranslations>(ports: &S) {
    let repo = ports.translations();
    stored(ports, product(1), "sr-Latn", "Šolja").await;
    stored(ports, product(1), "de", "Tasse").await;
    stored(ports, product(1), "de-AT", "Häferl").await;
    stored(ports, product(2), "de", "Teller").await;
    stored(ports, resource(ResourceKind::Category, 1), "de", "Tassen").await;

    let found = repo.for_resource(product(1)).await.unwrap();
    assert_eq!(
        keys_of(&found),
        [
            (product(1), "de".to_string()),
            (product(1), "de-AT".to_string()),
            (product(1), "sr-Latn".to_string()),
        ]
    );
    assert_eq!(repo.for_resource(product(9)).await, Ok(Vec::new()));
}

async fn loads_many<S: HasTranslations>(ports: &S) {
    let repo = ports.translations();
    let category = resource(ResourceKind::Category, 1);
    stored(ports, product(1), "de", "Tasse").await;
    stored(ports, product(1), "de-AT", "Häferl").await;
    stored(ports, product(1), "fr", "Tasse").await;
    stored(ports, product(2), "de", "Teller").await;
    stored(ports, product(3), "de", "Schale").await;
    stored(ports, category, "de", "Tassen").await;

    let found = repo
        .load(
            &[product(2), category, product(1), product(9)],
            &[lang("de-AT"), lang("de")],
        )
        .await
        .unwrap();
    assert_eq!(
        keys_of(&found),
        [
            (category, "de".to_string()),
            (product(1), "de".to_string()),
            (product(1), "de-AT".to_string()),
            (product(2), "de".to_string()),
        ]
    );
    assert_eq!(found[1].text("title"), Some("Tasse"));

    assert_eq!(repo.load(&[], &[lang("de")]).await, Ok(Vec::new()));
    assert_eq!(repo.load(&[product(1)], &[]).await, Ok(Vec::new()));
}

async fn overlay_falls_back<S: HasTranslations>(ports: &S) {
    let repo = ports.translations();
    let mut de = Translation::new(product(1), lang("de"));
    de.replace(
        &source(),
        fields(&[("title", "Tasse"), ("options.opt_1.name", "Farbe")]),
    )
    .unwrap();
    repo.insert(&mut de).await.unwrap();
    stored(ports, product(1), "de-AT", "Häferl").await;
    stored(ports, product(2), "de", "Teller").await;
    stored(ports, product(3), "fr", "Bol").await;

    let resources = [product(1), product(2), product(3)];
    let austrian = Overlay::load(ports, &lang("de-AT"), &resources)
        .await
        .unwrap();
    // The exact language first, field by field; then the language without its region.
    assert_eq!(austrian.text(product(1), "title"), Some("Häferl"));
    assert_eq!(
        austrian.text(product(1), "options.opt_1.name"),
        Some("Farbe")
    );
    assert_eq!(austrian.text(product(2), "title"), Some("Teller"));
    // Then none: the caller shows the source.
    assert_eq!(austrian.text(product(1), "description"), None);
    assert_eq!(austrian.text(product(3), "title"), None);
    assert_eq!(austrian.text(product(9), "title"), None);

    // Shoppers aren't served a fallback that isn't among the languages they may get.
    let unpublished = Overlay::load_within(ports, &lang("de-AT"), &[lang("de-AT")], &resources)
        .await
        .unwrap();
    assert_eq!(unpublished.text(product(1), "title"), Some("Häferl"));
    assert_eq!(unpublished.text(product(2), "title"), None);
    let published = [lang("de-AT"), lang("de")];
    let served = Overlay::load_within(ports, &lang("de-AT"), &published, &resources)
        .await
        .unwrap();
    assert_eq!(served.text(product(2), "title"), Some("Teller"));

    // A language never falls forward to a region's wording.
    let german = Overlay::load(ports, &lang("de"), &resources).await.unwrap();
    assert_eq!(german.text(product(1), "title"), Some("Tasse"));
    assert_eq!(Overlay::none().text(product(1), "title"), None);
}

async fn lists<S: HasTranslations>(ports: &S) {
    let repo = ports.translations();
    let category = resource(ResourceKind::Category, 1);
    let asset = resource(ResourceKind::Asset, 5);
    stored(ports, product(2), "de", "Teller").await;
    stored(ports, product(1), "sr-Latn", "Šolja").await;
    stored(ports, product(1), "de", "Tasse").await;
    stored(ports, category, "de", "Tassen").await;
    stored(ports, asset, "fr", "Une tasse").await;

    let everything = TranslationFilter::default();
    let all = repo
        .list(&everything, PageRequest::default())
        .await
        .unwrap();
    assert!(!all.has_more);
    let in_order = [
        (asset, "fr".to_string()),
        (category, "de".to_string()),
        (product(1), "de".to_string()),
        (product(1), "sr-Latn".to_string()),
        (product(2), "de".to_string()),
    ];
    assert_eq!(keys_of(&all.items), in_order);

    // Paged: each page continues after the last key of the one before.
    let two = PageLimit::new(2).unwrap();
    let first = repo
        .list(
            &everything,
            PageRequest {
                after: None,
                limit: two,
            },
        )
        .await
        .unwrap();
    assert!(first.has_more);
    assert_eq!(keys_of(&first.items), in_order[..2]);
    let second = repo
        .list(
            &everything,
            PageRequest {
                after: first.items.last().map(TranslationKey::of),
                limit: two,
            },
        )
        .await
        .unwrap();
    assert!(second.has_more);
    assert_eq!(keys_of(&second.items), in_order[2..4]);
    let third = repo
        .list(
            &everything,
            PageRequest {
                after: second.items.last().map(TranslationKey::of),
                limit: two,
            },
        )
        .await
        .unwrap();
    assert!(!third.has_more);
    assert_eq!(keys_of(&third.items), in_order[4..]);

    let of = |kinds: Option<Vec<ResourceKind>>, language: Option<&str>| TranslationFilter {
        kinds,
        language: language.map(lang),
    };
    let found = |filter: TranslationFilter| async move {
        keys_of(
            &repo
                .list(&filter, PageRequest::default())
                .await
                .unwrap()
                .items,
        )
    };
    assert_eq!(
        found(of(Some(vec![ResourceKind::Product]), None)).await,
        in_order[2..]
    );
    assert_eq!(
        found(of(
            Some(vec![ResourceKind::Asset, ResourceKind::Category]),
            None
        ))
        .await,
        in_order[..2]
    );
    assert_eq!(
        found(of(None, Some("de"))).await,
        [
            in_order[1].clone(),
            in_order[2].clone(),
            in_order[4].clone()
        ]
    );
    assert_eq!(
        found(of(Some(vec![ResourceKind::Product]), Some("sr-Latn"))).await,
        [in_order[3].clone()]
    );
    // No kinds is no translations, not every kind.
    assert_eq!(found(of(Some(Vec::new()), None)).await, []);
}

async fn any_in<S: HasTranslations>(ports: &S) {
    let repo = ports.translations();
    let both = [lang("de"), lang("fr")];
    assert_eq!(repo.any_in(&both).await, Ok(false));
    let mut t = translation(product(1), "de", "Tasse");
    repo.insert(&mut t).await.unwrap();
    assert_eq!(repo.any_in(&both).await, Ok(true));
    assert_eq!(repo.any_in(&[lang("fr")]).await, Ok(false));
    assert_eq!(repo.any_in(&[]).await, Ok(false));
    t.mark_deleted();
    repo.delete(&mut t).await.unwrap();
    assert_eq!(repo.any_in(&both).await, Ok(false));
}

async fn deletes_resources<S: HasTranslations + HasOutbox>(ports: &S) {
    let repo = ports.translations();
    let category = resource(ResourceKind::Category, 1);
    stored(ports, product(1), "de", "Tasse").await;
    stored(ports, product(1), "fr", "Tasse").await;
    stored(ports, product(2), "de", "Teller").await;
    stored(ports, product(3), "de", "Schale").await;
    // The same ID under another kind is another resource.
    stored(ports, category, "de", "Tassen").await;
    let before = ports.outbox().events_after(0, 100).await.unwrap().len();

    repo.delete_resources(&[product(1), product(3), product(9)])
        .await
        .unwrap();
    let left = repo
        .list(&TranslationFilter::default(), PageRequest::default())
        .await
        .unwrap();
    assert_eq!(
        keys_of(&left.items),
        [(category, "de".to_string()), (product(2), "de".to_string())]
    );
    // Again, and with nothing to remove: fine.
    repo.delete_resources(&[product(1)]).await.unwrap();
    repo.delete_resources(&[]).await.unwrap();
    let after = ports.outbox().events_after(0, 100).await.unwrap().len();
    assert_eq!(after, before);
}

async fn retains_languages<S: HasTranslations + HasOutbox>(ports: &S) {
    let repo = ports.translations();
    stored(ports, product(1), "de", "Tasse").await;
    stored(ports, product(1), "de-AT", "Häferl").await;
    stored(ports, product(1), "fr", "Tasse").await;
    stored(ports, product(2), "fr", "Assiette").await;
    let before = ports.outbox().events_after(0, 100).await.unwrap().len();

    // A language is kept by its exact tag: `de` doesn't keep `de-AT`.
    repo.retain_languages(&[lang("de"), lang("sr-Latn")])
        .await
        .unwrap();
    let left = repo
        .list(&TranslationFilter::default(), PageRequest::default())
        .await
        .unwrap();
    assert_eq!(keys_of(&left.items), [(product(1), "de".to_string())]);
    let after = ports.outbox().events_after(0, 100).await.unwrap().len();
    assert_eq!(after, before);

    // No languages on the list: every translation goes.
    repo.retain_languages(&[]).await.unwrap();
    assert_eq!(repo.any_in(&[lang("de"), lang("fr")]).await, Ok(false));
}
