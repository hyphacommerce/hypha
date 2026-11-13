//! What every translations store must do: one translation per resource and language,
//! read for many resources at once, listed in key order, removed with their resource or
//! their language.

use std::collections::BTreeMap;

use hypha_core::{
    HasOutbox, Language, OutboxRepo, PageLimit, PageRequest, RepoError, Stamped, Version, Versioned,
};
use hypha_translations::{
    FieldCounts, FieldInput, HasTranslations, Inheritance, LanguageProgress, Overlay,
    ProgressLanguage, ResourceKind, ResourceRef, Source, SourceDigest, SourceField, Translation,
    TranslationFilter, TranslationKey, TranslationRepo, TranslationSourceRepo, TranslationState,
    WorklistFilter, keys, source_digest, source_digests,
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
    scenario("translation sources: replaced whole per resource, none removes them");
    replaces_sources(&make().await).await;
    scenario("translation sources: the worklist counts fields in one language, by state");
    worklist_states(&make().await).await;
    scenario("translation sources: the worklist pages in the catalog's order of kinds");
    worklist_pages(&make().await).await;
    scenario("translation sources: progress counts each language's fields");
    progress(&make().await).await;
    scenario("translation sources: a regional language counts what it inherits from its base");
    inherits(&make().await).await;
    scenario(
        "translation sources: the queue, longest waiting first, queued again survives, counted",
    );
    queue(&make().await).await;
}

fn digest(key: &str, text: &str) -> SourceDigest {
    SourceDigest {
        key: key.to_string(),
        digest: source_digest(text),
    }
}

/// The digests of `source()`'s fields with text, as a product's.
fn product_sources() -> Vec<SourceDigest> {
    source_digests(&Source::new(vec![
        SourceField::new("title", Some("Mug")),
        SourceField::new("description", Some("<p>A mug.</p>")),
        SourceField::new("options.opt_1.name", Some("Colour")),
    ]))
}

/// A stored translation of `resource` into `language` of `fields`, against `source()`.
async fn translated<S: HasTranslations>(
    ports: &S,
    resource: ResourceRef,
    language: &str,
    values: &[(&str, &str)],
) {
    let mut t = Translation::new(resource, lang(language));
    t.replace(&source(), fields(values)).unwrap();
    ports.translations().insert(&mut t).await.unwrap();
}

async fn replaces_sources<S: HasTranslations + HasOutbox>(ports: &S) {
    let repo = ports.translations();
    let category = resource(ResourceKind::Category, 1);
    repo.replace_sources(product(1), &product_sources())
        .await
        .unwrap();
    repo.replace_sources(category, &[digest("name", "Mugs")])
        .await
        .unwrap();
    let mut both = product_sources()
        .into_iter()
        .map(|d| (product(1), d))
        .collect::<Vec<_>>();
    both.insert(0, (category, digest("name", "Mugs")));
    // By resource (the catalog's order of kinds: products first), then key.
    both.sort();
    assert_eq!(repo.sources(&[product(1), category]).await, Ok(both));

    // Replaced whole: a key left out goes, a changed digest changes.
    let fewer = vec![digest("title", "Cup")];
    repo.replace_sources(product(1), &fewer).await.unwrap();
    assert_eq!(
        repo.sources(&[product(1)]).await,
        Ok(vec![(product(1), digest("title", "Cup"))])
    );
    // None removes them; another resource's stay.
    repo.replace_sources(product(1), &[]).await.unwrap();
    assert_eq!(repo.sources(&[product(1)]).await, Ok(vec![]));
    assert_eq!(repo.sources(&[category]).await.unwrap().len(), 1);
    assert_eq!(repo.sources(&[]).await, Ok(vec![]));
    // A read model: no events.
    assert_eq!(ports.outbox().events_after(0, 100).await.unwrap().len(), 0);
}

async fn worklist_states<S: HasTranslations>(ports: &S) {
    let repo = ports.translations();
    // 1 complete, 2 outdated (its title changed since), 3 untranslated, 4 half done and
    // its other field changed: missing outranks outdated.
    for n in 1..=4 {
        repo.replace_sources(product(n), &product_sources())
            .await
            .unwrap();
    }
    let all = [
        ("title", "Tasse"),
        ("description", "<p>Eine Tasse.</p>"),
        ("options.opt_1.name", "Farbe"),
    ];
    translated(ports, product(1), "de", &all).await;
    translated(ports, product(2), "de", &all).await;
    translated(ports, product(4), "de", &all[..1]).await;
    let changed = |n: u128| {
        let mut d = product_sources();
        for s in &mut d {
            if s.key == "title" {
                s.digest = source_digest("Cup");
            }
        }
        (product(n), d)
    };
    for (r, d) in [changed(2), changed(4)] {
        repo.replace_sources(r, &d).await.unwrap();
    }
    // A translation in another language and a resource without text count for nothing.
    translated(ports, product(3), "fr", &all).await;
    repo.replace_sources(resource(ResourceKind::Asset, 9), &[])
        .await
        .unwrap();

    let list = async |state: Option<TranslationState>| {
        let filter = WorklistFilter {
            kinds: ResourceKind::ALL.to_vec(),
            language: lang("de"),
            inherits: Inheritance::Nothing,
            state,
        };
        repo.worklist(&filter, PageRequest::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|item| (item.resource, item.counts, item.translation.is_some()))
            .collect::<Vec<_>>()
    };
    let counts = |missing, outdated| FieldCounts {
        total: 3,
        missing,
        outdated,
        inherited: 0,
    };
    assert_eq!(
        list(Some(TranslationState::Missing)).await,
        [
            (product(3), counts(3, 0), false),
            (product(4), counts(2, 1), true)
        ]
    );
    assert_eq!(
        list(Some(TranslationState::Outdated)).await,
        [(product(2), counts(0, 1), true)]
    );
    assert_eq!(
        list(Some(TranslationState::Complete)).await,
        [(product(1), counts(0, 0), true)]
    );
    assert_eq!(list(None).await.len(), 4);

    // The item carries the translation, as stored.
    let filter = WorklistFilter {
        kinds: vec![ResourceKind::Product],
        language: lang("de"),
        inherits: Inheritance::Nothing,
        state: Some(TranslationState::Outdated),
    };
    let page = repo
        .worklist(&filter, PageRequest::default())
        .await
        .unwrap();
    assert_eq!(
        page.items[0].translation,
        repo.get(product(2), &lang("de")).await.unwrap()
    );
    // Kinds not asked for aren't listed.
    let categories = WorklistFilter {
        kinds: vec![ResourceKind::Category],
        ..filter
    };
    assert!(
        repo.worklist(&categories, PageRequest::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

async fn worklist_pages<S: HasTranslations>(ports: &S) {
    let repo = ports.translations();
    let category = resource(ResourceKind::Category, 1);
    let asset = resource(ResourceKind::Asset, 1);
    // The catalog lists products, then categories, …, then assets: not their names'
    // order. IDs by their bytes within a kind.
    for r in [asset, category, product(2), product(1)] {
        repo.replace_sources(r, &[digest("title", "Mug")])
            .await
            .unwrap();
    }
    let filter = WorklistFilter {
        kinds: ResourceKind::ALL.to_vec(),
        language: lang("de"),
        inherits: Inheritance::Nothing,
        state: Some(TranslationState::Missing),
    };
    let in_order = [product(1), product(2), category, asset];
    let one = PageLimit::new(1).unwrap();
    let mut after = None;
    let mut seen = Vec::new();
    loop {
        let page = repo
            .worklist(&filter, PageRequest { after, limit: one })
            .await
            .unwrap();
        seen.extend(page.items.iter().map(|i| i.resource));
        if !page.has_more {
            break;
        }
        after = page.items.last().map(|i| i.resource);
    }
    assert_eq!(seen, in_order);
    let all = repo
        .worklist(&filter, PageRequest::default())
        .await
        .unwrap();
    assert!(!all.has_more);
    assert_eq!(
        all.items.iter().map(|i| i.resource).collect::<Vec<_>>(),
        in_order
    );
}

async fn progress<S: HasTranslations>(ports: &S) {
    let repo = ports.translations();
    let category = resource(ResourceKind::Category, 1);
    repo.replace_sources(product(1), &product_sources())
        .await
        .unwrap();
    repo.replace_sources(product(2), &product_sources())
        .await
        .unwrap();
    repo.replace_sources(category, &[digest("name", "Mugs")])
        .await
        .unwrap();
    let de = [("title", "Tasse"), ("options.opt_1.name", "Farbe")];
    translated(ports, product(1), "de", &de).await;
    translated(ports, product(2), "de", &de[..1]).await;
    translated(ports, category, "de", &[("name", "Tassen")]).await;
    translated(ports, product(1), "fr", &de[..1]).await;
    // Product 2's title changed since; product 1 lost its option, whose translation now
    // counts for nothing.
    let mut changed = product_sources();
    for s in &mut changed {
        if s.key == "title" {
            s.digest = source_digest("Cup");
        }
    }
    repo.replace_sources(product(2), &changed).await.unwrap();
    let without_option: Vec<SourceDigest> = product_sources()
        .into_iter()
        .filter(|d| d.key != "options.opt_1.name")
        .collect();
    repo.replace_sources(product(1), &without_option)
        .await
        .unwrap();

    let of = |language: &str, total, translated, outdated| LanguageProgress {
        language: lang(language),
        total,
        translated,
        outdated,
        inherited: 0,
    };
    assert_eq!(
        repo.progress(&ResourceKind::ALL, &plain(&["fr", "de", "it"]))
            .await,
        // In the order asked, languages without translations too.
        Ok(vec![
            of("fr", 6, 1, 0),
            of("de", 6, 3, 1),
            of("it", 6, 0, 0)
        ])
    );
    assert_eq!(
        repo.progress(&[ResourceKind::Category], &plain(&["de"]))
            .await,
        Ok(vec![of("de", 1, 1, 0)])
    );
    assert_eq!(
        repo.progress(&[], &plain(&["de"])).await,
        Ok(vec![of("de", 0, 0, 0)])
    );
    assert_eq!(repo.progress(&ResourceKind::ALL, &[]).await, Ok(vec![]));
    assert_eq!(of("de", 6, 3, 1).missing(), 3);
}

/// Languages counted with nothing to inherit.
fn plain(tags: &[&str]) -> Vec<ProgressLanguage> {
    tags.iter()
        .map(|tag| ProgressLanguage {
            language: lang(tag),
            inherits: Inheritance::Nothing,
        })
        .collect()
}

async fn inherits<S: HasTranslations>(ports: &S) {
    let repo = ports.translations();
    let category = resource(ResourceKind::Category, 1);
    for n in 1..=3 {
        repo.replace_sources(product(n), &product_sources())
            .await
            .unwrap();
    }
    repo.replace_sources(category, &[digest("name", "Mugs")])
        .await
        .unwrap();
    // German translates product 1 whole, product 2's title and the category; Austrian
    // German has product 1's title of its own and product 3's option, which German lacks.
    let all = [
        ("title", "Tasse"),
        ("description", "<p>Eine Tasse.</p>"),
        ("options.opt_1.name", "Farbe"),
    ];
    translated(ports, product(1), "de", &all).await;
    translated(ports, product(2), "de", &all[..1]).await;
    translated(ports, category, "de", &[("name", "Tassen")]).await;
    translated(ports, product(1), "de-AT", &[("title", "Häferl")]).await;
    translated(
        ports,
        product(3),
        "de-AT",
        &[("options.opt_1.name", "Farbe")],
    )
    .await;

    let from_german = Inheritance::Translation(lang("de"));
    let counted = |tag: &str, inherits: Inheritance| ProgressLanguage {
        language: lang(tag),
        inherits,
    };
    let progress = repo
        .progress(
            &ResourceKind::ALL,
            &[
                counted("de-AT", from_german.clone()),
                counted("en-GB", Inheritance::Source),
                counted("de", Inheritance::Nothing),
            ],
        )
        .await
        .unwrap();
    let inherited: Vec<(u64, u64)> = progress
        .iter()
        .map(|p| (p.missing(), p.inherited))
        .collect();
    // 10 fields. de-AT: 2 its own, of the 8 missing German has product 1's other two,
    // product 2's title and the category's name. en-GB inherits all it lacks.
    assert_eq!(inherited, [(8, 4), (10, 10), (5, 0)]);
    assert_eq!(
        repo.progress(
            &[ResourceKind::Category],
            &[counted("de-AT", from_german.clone())]
        )
        .await
        .unwrap()[0]
            .inherited,
        1
    );

    let filter = WorklistFilter {
        kinds: vec![ResourceKind::Product],
        language: lang("de-AT"),
        inherits: from_german,
        state: None,
    };
    let items: Vec<(ResourceRef, u32, u32)> = repo
        .worklist(&filter, PageRequest::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|i| (i.resource, i.counts.missing, i.counts.inherited))
        .collect();
    assert_eq!(
        items,
        [(product(1), 2, 2), (product(2), 3, 1), (product(3), 2, 0)]
    );
    let from_source = WorklistFilter {
        inherits: Inheritance::Source,
        language: lang("en-GB"),
        ..filter
    };
    let first = &repo
        .worklist(&from_source, PageRequest::default())
        .await
        .unwrap()
        .items[0];
    assert_eq!((first.counts.missing, first.counts.inherited), (3, 3));
}

async fn queue<S: HasTranslations>(ports: &S) {
    let repo = ports.translations();
    let category = resource(ResourceKind::Category, 1);
    assert_eq!(repo.queued(10).await, Ok(vec![]));
    repo.queue(&[product(2), category]).await.unwrap();
    repo.queue(&[product(1)]).await.unwrap();
    let resources = |queued: &[hypha_translations::QueuedSource]| {
        queued.iter().map(|q| q.resource).collect::<Vec<_>>()
    };
    let first = repo.queued(2).await.unwrap();
    assert_eq!(resources(&first).len(), 2);
    let all = repo.queued(10).await.unwrap();
    assert_eq!(all.len(), 3);
    // Counted by kind.
    assert_eq!(repo.pending(&ResourceKind::ALL).await, Ok(3));
    assert_eq!(repo.pending(&[ResourceKind::Category]).await, Ok(1));
    assert_eq!(repo.pending(&[]).await, Ok(0));
    // Longest waiting first.
    assert_eq!(resources(&all)[2], product(1));

    // Queued again after it was read: it stays queued, at the back.
    repo.queue(&[all[0].resource]).await.unwrap();
    repo.unqueue(&all).await.unwrap();
    let left = repo.queued(10).await.unwrap();
    assert_eq!(resources(&left), [all[0].resource]);
    repo.unqueue(&left).await.unwrap();
    assert_eq!(repo.queued(10).await, Ok(vec![]));
    assert_eq!(repo.pending(&ResourceKind::ALL).await, Ok(0));

    // Touched while queued: it stays queued, at the back; one not queued stays off it.
    repo.queue(&[product(1)]).await.unwrap();
    let read = repo.queued(10).await.unwrap();
    repo.touch_queued(&[product(1), product(2)]).await.unwrap();
    repo.unqueue(&read).await.unwrap();
    assert_eq!(resources(&repo.queued(10).await.unwrap()), [product(1)]);
    let read = repo.queued(10).await.unwrap();
    repo.unqueue(&read).await.unwrap();
    repo.touch_queued(&[]).await.unwrap();
    assert_eq!(repo.queued(10).await, Ok(vec![]));

    // Every resource of a kind that has digests.
    repo.replace_sources(product(1), &product_sources())
        .await
        .unwrap();
    repo.replace_sources(product(2), &product_sources())
        .await
        .unwrap();
    repo.replace_sources(category, &[digest("name", "Mugs")])
        .await
        .unwrap();
    repo.queue_kind(ResourceKind::Product).await.unwrap();
    let mut queued = resources(&repo.queued(10).await.unwrap());
    queued.sort();
    assert_eq!(queued, [product(1), product(2)]);
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
    let all = [lang("de"), lang("de-AT"), lang("fr")];
    let austrian = Overlay::load(ports, &lang("de-AT"), &all, &resources)
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

    // A fallback that may not be served isn't: `de-AT` published, `de` not.
    let published = [lang("de-AT")];
    let shopper = Overlay::load(ports, &lang("de-AT"), &published, &resources)
        .await
        .unwrap();
    assert_eq!(shopper.text(product(1), "title"), Some("Häferl"));
    assert_eq!(shopper.text(product(1), "options.opt_1.name"), None);
    assert_eq!(shopper.text(product(2), "title"), None);
    // Nor is the language asked for, when it may not be served; its fallback still is.
    let only_german = Overlay::load(ports, &lang("de-AT"), &[lang("de")], &resources)
        .await
        .unwrap();
    assert_eq!(only_german.text(product(1), "title"), Some("Tasse"));
    let nothing = Overlay::load(ports, &lang("de-AT"), &[], &resources)
        .await
        .unwrap();
    assert_eq!(nothing.text(product(1), "title"), None);

    // A language never falls forward to a region's wording.
    let german = Overlay::load(ports, &lang("de"), &all, &resources)
        .await
        .unwrap();
    assert_eq!(german.text(product(1), "title"), Some("Tasse"));
    assert_eq!(Overlay::none().text(product(1), "title"), None);

    // What a reader is shown: the translation, else the source it hands over.
    assert_eq!(austrian.text_or(product(1), "title", "Mug"), "Häferl");
    assert_eq!(
        austrian.text_or(product(1), "description", "<p>Mug</p>"),
        "<p>Mug</p>"
    );
    assert_eq!(
        austrian.optional(product(1), "title", Some("Mug")),
        Some("Häferl")
    );
    assert_eq!(
        austrian.optional(product(1), "description", Some("x")),
        Some("x")
    );
    // A field the resource lacks now has nothing to show, translated or not.
    assert_eq!(austrian.optional(product(1), "title", None), None);
    assert!(austrian.translates(product(1)));
    assert!(!austrian.translates(product(9)));
    assert!(!Overlay::none().translates(product(1)));
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
