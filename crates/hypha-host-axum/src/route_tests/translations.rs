//! Translations: their routes, the languages they may be in, and what removes them.

use serde_json::{Value, json};

use super::*;

/// English content with German (published), Austrian German and French beside it.
async fn in_four_languages(app: &TestApp) {
    let body = r#"{"languages":[{"language":"de","published":true},{"language":"de-AT"},{"language":"fr"}]}"#;
    let (status, store) = send(app, "PATCH", "/admin/store", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{store}");
}

async fn store_id(app: &TestApp) -> String {
    let (_, store) = send(app, "GET", "/admin/store", None).await;
    store["id"].as_str().unwrap().to_string()
}

/// `{"fields": {key: {"value": value}}}`.
fn fields(fields: &[(&str, &str)]) -> String {
    let fields: serde_json::Map<String, Value> = fields
        .iter()
        .map(|(key, value)| (key.to_string(), json!({ "value": value })))
        .collect();
    json!({ "fields": fields }).to_string()
}

async fn translate(app: &TestApp, path: &str, language: &str, body: &str) -> (StatusCode, Value) {
    send(app, "PUT", &format!("{path}/{language}"), Some(body)).await
}

fn field<'a>(view: &'a Value, key: &str) -> &'a Value {
    view["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == key)
        .unwrap_or_else(|| panic!("no field {key} in {view}"))
}

fn keys(view: &Value) -> Vec<&str> {
    view["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap())
        .collect()
}

fn problems(body: &Value) -> Vec<(&str, &str)> {
    body["details"]["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["key"].as_str().unwrap(), p["code"].as_str().unwrap()))
        .collect()
}

#[tokio::test]
async fn a_products_translation_is_written_read_and_goes_outdated() {
    let app = app().await;
    in_four_languages(&app).await;
    let product = create_product(&app, "mug").await;
    let options = r#"{"options":[{"name":"Size","values":[{"label":"S"},{"label":"M"}]}]}"#;
    let (_, sized) = send(
        &app,
        "PUT",
        &format!("/admin/products/{product}/options"),
        Some(options),
    )
    .await;
    let option = sized["options"][0]["id"].as_str().unwrap().to_string();
    let small = sized["options"][0]["values"][0]["id"].as_str().unwrap();
    let path = format!("/admin/translations/product/{product}");

    // Before any translation: the source, its digest, what it can be translated into.
    let (status, view) = send(&app, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["resource"], json!({"kind": "product", "id": product}));
    assert_eq!(view["default_language"], "en");
    assert_eq!(view["languages"], json!(["de", "de-AT", "fr"]));
    assert_eq!(view["translations"], json!([]));
    let option_key = format!("options.{option}.name");
    let small_key = format!("option_values.{small}.label");
    assert_eq!(keys(&view).len(), 5, "{view}");
    assert_eq!(
        keys(&view)[..3],
        ["title", "description", option_key.as_str()]
    );
    assert!(keys(&view).contains(&small_key.as_str()));
    let title = field(&view, "title");
    assert_eq!(
        (&title["source"], &title["format"], &title["max_length"]),
        (&json!("mug"), &json!("text"), &json!(255))
    );
    assert_eq!(title["translations"], json!({}));
    assert_eq!(
        (
            &field(&view, "description")["source"],
            &field(&view, "description")["format"]
        ),
        (&json!(""), &json!("html"))
    );
    let digest = title["digest"].as_str().unwrap().to_string();
    assert_eq!(digest.len(), 64);

    let body = json!({"fields": {
        "title": {"value": "  Tasse ", "source_digest": digest},
        "description": {"value": "<p>Eine Tasse.</p><script>alert(1)</script>"},
        &option_key: {"value": "Größe"},
    }})
    .to_string();
    let (status, view) = translate(&app, &path, "DE", &body).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        field(&view, "title")["translations"]["de"],
        json!({"value": "Tasse", "source_digest": digest, "outdated": false})
    );
    assert_eq!(
        field(&view, "description")["translations"]["de"]["value"],
        "<p>Eine Tasse.</p>"
    );
    assert_eq!(view["translations"][0]["language"], "de");
    assert_eq!(view["translations"][0]["translated"], 3);
    assert_eq!(view["translations"][0]["outdated"], 0);
    let event = outbox_events(&app).await.pop().unwrap();
    assert_eq!(event.event_type, "translation.updated");
    assert_eq!(
        event.data,
        json!({
            "resource": {"kind": "product", "id": product},
            "language": "de",
            "fields": ["description", option_key, "title"],
        })
    );

    // The source changes: the translation stays, marked outdated.
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/admin/products/{product}"),
        Some(r#"{"title":"Cup"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, view) = send(&app, "GET", &path, None).await;
    let title = field(&view, "title");
    assert_eq!(title["source"], "Cup");
    assert_ne!(title["digest"], digest.as_str());
    assert_eq!(
        title["translations"]["de"],
        json!({"value": "Tasse", "source_digest": digest, "outdated": true})
    );
    assert_eq!(view["translations"][0]["outdated"], 1);

    // Saving the same text again keeps it outdated; the digest read with the source
    // confirms it; a digest of the text before is refused.
    let same = fields(&[
        ("title", "Tasse"),
        ("description", "<p>Eine Tasse.</p>"),
        (&option_key, "Größe"),
    ]);
    let (_, view) = translate(&app, &path, "de", &same).await;
    assert_eq!(
        field(&view, "title")["translations"]["de"]["outdated"],
        true
    );
    let stale = json!({"fields": {"title": {"value": "Tasse", "source_digest": digest}}});
    let (status, refused) = translate(&app, &path, "de", &stale.to_string()).await;
    assert_eq!(
        (status, code(&refused)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_translation")
    );
    assert_eq!(problems(&refused), [("title", "source_changed")]);
    let now = field(&view, "title")["digest"].clone();
    let confirmed = json!({"fields": {
        "title": {"value": "Tasse", "source_digest": now},
        &option_key: {"value": "Größe"},
    }});
    let (status, view) = translate(&app, &path, "de", &confirmed.to_string()).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        field(&view, "title")["translations"]["de"]["outdated"],
        false
    );
    // The description was left out of that write: it's removed.
    assert_eq!(field(&view, "description")["translations"], json!({}));

    // An option that's gone: its translation isn't shown, and can't be written.
    let (status, _) = send(
        &app,
        "PUT",
        &format!("/admin/products/{product}/options"),
        Some(r#"{"options":[]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, view) = send(&app, "GET", &path, None).await;
    assert_eq!(keys(&view), ["title", "description"]);
    assert_eq!(view["translations"][0]["translated"], 1);
    let gone = fields(&[("title", "Tasse"), (&option_key, "Größe")]);
    let (status, refused) = translate(&app, &path, "de", &gone).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(problems(&refused), [(option_key.as_str(), "unknown_child")]);

    // No fields left deletes the translation.
    let (status, view) = translate(&app, &path, "de", &fields(&[("title", " ")])).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["translations"], json!([]));
    let event = outbox_events(&app).await.pop().unwrap();
    assert_eq!(event.event_type, "translation.deleted");
}

#[tokio::test]
async fn a_translation_is_only_into_one_of_the_stores_other_languages() {
    let app = app().await;
    in_four_languages(&app).await;
    let product = create_product(&app, "mug").await;
    let path = format!("/admin/translations/product/{product}");
    let tasse = fields(&[("title", "Tasse")]);

    let refused = async |language: &str| {
        let (status, body) = translate(&app, &path, language, &tasse).await;
        (status, code(&body).to_string())
    };
    let unprocessable = StatusCode::UNPROCESSABLE_ENTITY;
    assert_eq!(
        refused("en").await,
        (
            unprocessable,
            "translation_into_default_language".to_string()
        )
    );
    assert_eq!(
        refused("es").await,
        (unprocessable, "language_not_listed".to_string())
    );
    assert_eq!(
        refused("de-CH").await,
        (unprocessable, "language_not_listed".to_string())
    );
    assert_eq!(
        refused("german").await,
        (unprocessable, "invalid_language".to_string())
    );
    // An unpublished language can be translated into.
    let (status, view) = translate(&app, &path, "fr", &tasse).await;
    assert_eq!(status, StatusCode::OK, "{view}");

    // Every problem of the fields at once.
    let long = "a".repeat(256);
    let body = fields(&[("title", &long), ("handle", "tasse")]);
    let (status, body) = translate(&app, &path, "de", &body).await;
    assert_eq!(
        (status, code(&body)),
        (unprocessable, "invalid_translation")
    );
    assert_eq!(
        problems(&body),
        [("handle", "unknown_field"), ("title", "too_long")]
    );

    // The resource and its kind come from the URL.
    let missing = "/admin/translations/product/prod_00000000000000000000000000000001";
    let (status, body) = send(&app, "GET", missing, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "resource_not_found")
    );
    let (status, body) = translate(&app, missing, "de", &tasse).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "resource_not_found")
    );
    let (status, body) = send(
        &app,
        "GET",
        &format!("/admin/translations/variant/{product}"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "unknown_resource_kind")
    );
    let (status, body) = send(
        &app,
        "GET",
        &format!("/admin/translations/category/{product}"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_path")
    );

    // Deleting one language leaves the others.
    let (status, _) = translate(&app, &path, "de", &tasse).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(&app, "DELETE", &format!("{path}/de"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(&app, "DELETE", &format!("{path}/de"), None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "translation_not_found")
    );
    let (_, view) = send(&app, "GET", &path, None).await;
    assert_eq!(view["translations"].as_array().unwrap().len(), 1);
    assert_eq!(view["translations"][0]["language"], "fr");
}

#[tokio::test]
async fn translating_takes_the_resources_read_scope_too() {
    let app = app().await;
    in_four_languages(&app).await;
    let product = create_product(&app, "mug").await;
    let path = format!("/admin/translations/product/{product}");
    let tasse = fields(&[("title", "Tasse")]);
    let with = async |name: &str, scopes: Value| {
        key(
            &app,
            json!({ "kind": "secret", "name": name, "scopes": scopes }),
        )
        .await
    };
    let translator = with("Translator", json!(["translations:write", "products:read"])).await;
    let reader = with("Reader", json!(["translations:read", "products:read"])).await;
    let blind = with("Blind", json!(["translations:write"])).await;
    let forbidden = (StatusCode::FORBIDDEN, "missing_scope".to_string());
    let as_key = async |token: &str, method: &str, uri: &str, body: Option<&str>| {
        let (status, body) = send_as(&app, token, method, uri, body).await;
        (status, code(&body).to_string())
    };
    let de = format!("{path}/de");

    assert_eq!(as_key(&blind, "GET", &path, None).await, forbidden);
    assert_eq!(as_key(&blind, "PUT", &de, Some(&tasse)).await, forbidden);
    // Without the product's scope not even whether it exists.
    let missing = "/admin/translations/product/prod_00000000000000000000000000000001";
    assert_eq!(as_key(&blind, "GET", missing, None).await, forbidden);
    assert_eq!(as_key(&reader, "PUT", &de, Some(&tasse)).await, forbidden);
    assert_eq!(as_key(&reader, "DELETE", &de, None).await, forbidden);

    assert_eq!(
        as_key(&translator, "PUT", &de, Some(&tasse)).await.0,
        StatusCode::OK
    );
    assert_eq!(as_key(&reader, "GET", &path, None).await.0, StatusCode::OK);
    // Another kind takes its own scope.
    let store = format!("/admin/translations/store/{}", store_id(&app).await);
    assert_eq!(as_key(&reader, "GET", &store, None).await, forbidden);

    // The list shows the kinds the caller reads, and takes a kind's scope by name.
    let (status, page) = send_as(&app, &reader, "GET", "/admin/translations", None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        as_key(&reader, "GET", "/admin/translations?kind=discount", None).await,
        forbidden
    );
    assert_eq!(
        as_key(&blind, "GET", "/admin/translations", None).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn every_kind_has_its_fields() {
    let app = app().await;
    in_four_languages(&app).await;
    let created = async |uri: &str, body: Value| {
        let (status, json) = send(&app, "POST", uri, Some(&body.to_string())).await;
        assert_eq!(status, StatusCode::CREATED, "{uri}: {json}");
        json
    };
    let translated = async |kind: &str, id: &str, key: &str, source: &str| {
        let path = format!("/admin/translations/{kind}/{id}");
        let (status, view) = send(&app, "GET", &path, None).await;
        assert_eq!(status, StatusCode::OK, "{kind}: {view}");
        assert_eq!(field(&view, key)["source"], source, "{kind} {key}");
        let (status, view) = translate(&app, &path, "de", &fields(&[(key, "Deutsch")])).await;
        assert_eq!(status, StatusCode::OK, "{kind}: {view}");
        assert_eq!(
            field(&view, key)["translations"]["de"]["value"],
            "Deutsch",
            "{kind} {key}"
        );
        keys(&view)
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let id = |json: &Value| json["id"].as_str().unwrap().to_string();

    let category = created(
        "/admin/categories",
        json!({"name": "Mugs", "handle": "mugs", "visible": true, "description": "<p>For coffee.</p>"}),
    )
    .await;
    assert_eq!(
        translated(
            "category",
            &id(&category),
            "description",
            "<p>For coffee.</p>"
        )
        .await,
        ["name", "description"]
    );

    let definition = created(
        "/admin/custom-field-definitions",
        json!({
            "owner_type": "product", "key": "material",
            "name": "Material", "type": "choice",
            "validations": {"choices": [{"key": "wool", "label": "Wool"}]},
        }),
    )
    .await;
    assert_eq!(
        translated(
            "custom_field_definition",
            &id(&definition),
            "choices.wool.label",
            "Wool"
        )
        .await,
        ["name", "description", "choices.wool.label"]
    );

    let zone = created(
        "/admin/shipping-zones",
        json!({"name": "Croatia", "countries": ["HR"]}),
    )
    .await;
    let option = json!({
        "name": "Standard", "code": "standard",
        "rate": {"type": "builtin", "basis": "subtotal", "tables": [
            {"currency": "EUR", "tiers": [{"amount": 490}]}
        ]},
    });
    let zone_path = format!("/admin/shipping-zones/{}", id(&zone));
    let zone = created(&format!("{zone_path}/options"), option).await;
    let option = zone["options"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(
        translated("shipping_option", &option, "name", "Standard").await,
        ["name"]
    );

    let method = created(
        "/admin/payment-methods",
        json!({"provider": "manual", "name": "Cash on delivery", "instructions": "Pay the courier."}),
    )
    .await;
    assert_eq!(
        translated(
            "payment_method",
            &id(&method),
            "instructions",
            "Pay the courier."
        )
        .await,
        ["name", "instructions"]
    );

    let rule = created(
        "/admin/checkout-rules",
        json!({
            "name": "No PO boxes",
            "conditions": [{"type": "address_is", "flag": "po_box"}],
            "outcome": {"severity": "error", "message": "We can't ship to PO boxes."},
        }),
    )
    .await;
    assert_eq!(
        translated(
            "checkout_rule",
            &id(&rule),
            "message",
            "We can't ship to PO boxes."
        )
        .await,
        ["message"]
    );

    // The store's own text, and its search facets, are named by the store's ID.
    let store = store_id(&app).await;
    let (status, _) = send(
        &app,
        "PATCH",
        "/admin/store",
        Some(r#"{"description":"Mugs for mornings."}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        translated("store", &store, "description", "Mugs for mornings.").await,
        ["description", "email_footer"]
    );
    let (status, body) = send(
        &app,
        "GET",
        "/admin/translations/store/store_00000000000000000000000000000001",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "resource_not_found")
    );

    let (status, settings) = send(
        &app,
        "PATCH",
        "/admin/search/settings",
        Some(r#"{"facets":[{"key":"vendor","label":"Maker"},{"key":"option.Size"}]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    assert_eq!(
        translated("search_facet", &store, "facets.vendor.label", "Maker").await,
        ["facets.vendor.label", "facets.option.size.label"]
    );

    // Deleting what no event names takes its translations along.
    let listed = async || {
        let (_, page) = send(&app, "GET", "/admin/translations?limit=100", None).await;
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["resource"]["kind"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        listed().await,
        [
            "category",
            "checkout_rule",
            "custom_field_definition",
            "payment_method",
            "search_facet",
            "shipping_option",
            "store"
        ]
    );
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/admin/payment-methods/{}", id(&method)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&app, "DELETE", &zone_path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        listed().await,
        [
            "category",
            "checkout_rule",
            "custom_field_definition",
            "search_facet",
            "store"
        ]
    );

    // And what an event names goes when the event is delivered.
    for uri in [
        format!("/admin/categories/{}", id(&category)),
        format!("/admin/checkout-rules/{}", id(&rule)),
        format!("/admin/custom-field-definitions/{}", id(&definition)),
    ] {
        let (status, body) = send(&app, "DELETE", &uri, None).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{uri}: {body}");
    }
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    assert_eq!(listed().await, ["search_facet", "store"]);
}

#[tokio::test]
async fn translations_are_listed_by_kind_and_language_in_pages() {
    let app = app().await;
    in_four_languages(&app).await;
    let mug = create_product(&app, "mug").await;
    let plate = create_product(&app, "plate").await;
    for (product, language) in [(&mug, "de"), (&mug, "fr"), (&plate, "de")] {
        let path = format!("/admin/translations/product/{product}");
        let (status, body) = translate(&app, &path, language, &fields(&[("title", "X")])).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let store = store_id(&app).await;
    let path = format!("/admin/translations/store/{store}");
    let (status, _) = translate(&app, &path, "de", &fields(&[("description", "Tassen")])).await;
    assert_eq!(status, StatusCode::OK);

    let list = async |query: &str| {
        let (status, page) = send(&app, "GET", &format!("/admin/translations{query}"), None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page
    };
    let all = list("").await;
    assert_eq!(all["items"].as_array().unwrap().len(), 4);
    assert_eq!(all["has_more"], false);
    assert_eq!(
        all["items"][3],
        json!({
            "resource": {"kind": "store", "id": store},
            "language": "de",
            "fields": ["description"],
            "created_at": all["items"][3]["created_at"],
            "updated_at": all["items"][3]["updated_at"],
        })
    );

    assert_eq!(
        list("?kind=product").await["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        list("?language=fr").await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        list("?kind=product&language=DE").await["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Paged, each page continuing the one before.
    let first = list("?limit=3").await;
    assert_eq!(first["has_more"], true);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = list(&format!("?limit=3&cursor={cursor}")).await;
    assert_eq!(second["has_more"], false);
    assert_eq!(second["items"][0]["resource"]["kind"], "store");

    for (query, expected) in [
        ("?kind=variant", "invalid_query"),
        ("?language=de_DE", "invalid_query"),
        ("?state=missing", "invalid_query"),
    ] {
        let (status, body) = send(&app, "GET", &format!("/admin/translations{query}"), None).await;
        assert_eq!(
            (status, code(&body)),
            (StatusCode::BAD_REQUEST, expected),
            "{query}"
        );
    }
}

#[tokio::test]
async fn a_deleted_products_translations_go_with_it() {
    let app = app().await;
    in_four_languages(&app).await;
    let mug = create_product(&app, "mug").await;
    let plate = create_product(&app, "plate").await;
    for product in [&mug, &plate] {
        let path = format!("/admin/translations/product/{product}");
        let (status, _) = translate(&app, &path, "de", &fields(&[("title", "X")])).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, _) = send(&app, "DELETE", &format!("/admin/products/{mug}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    hypha_http::jobs::dispatch(&app.state).await.unwrap();

    let (_, page) = send(&app, "GET", "/admin/translations", None).await;
    let left: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["resource"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(left, [plate.as_str()]);
}

#[tokio::test]
async fn a_language_taken_off_the_stores_list_takes_its_translations() {
    let app = app().await;
    in_four_languages(&app).await;
    let product = create_product(&app, "mug").await;
    let path = format!("/admin/translations/product/{product}");
    for language in ["de", "de-AT", "fr"] {
        let (status, _) = translate(&app, &path, language, &fields(&[("title", "X")])).await;
        assert_eq!(status, StatusCode::OK);
    }

    // Unpublishing keeps the language on the list, and its translations.
    let body = r#"{"languages":[{"language":"de"},{"language":"fr"}]}"#;
    let (status, store) = send(&app, "PATCH", "/admin/store", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{store}");
    hypha_http::jobs::dispatch(&app.state).await.unwrap();

    let (_, view) = send(&app, "GET", &path, None).await;
    let languages: Vec<&str> = view["translations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["language"].as_str().unwrap())
        .collect();
    assert_eq!(languages, ["de", "fr"]);
    assert_eq!(view["languages"], json!(["de", "fr"]));

    // A change of the store that isn't its languages removes nothing.
    let (status, _) = send(&app, "PATCH", "/admin/store", Some(r#"{"name":"Lumen"}"#)).await;
    assert_eq!(status, StatusCode::OK);
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let (_, view) = send(&app, "GET", &path, None).await;
    assert_eq!(view["translations"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn the_default_language_is_fixed_while_anything_is_translated() {
    let app = app().await;
    in_four_languages(&app).await;
    let product = create_product(&app, "mug").await;
    let path = format!("/admin/translations/product/{product}");
    let (status, _) = translate(&app, &path, "fr", &fields(&[("title", "Tasse")])).await;
    assert_eq!(status, StatusCode::OK);

    let patch = async |body: &str| {
        let (status, json) = send(&app, "PATCH", "/admin/store", Some(body)).await;
        (status, code(&json).to_string())
    };
    let in_use = (StatusCode::CONFLICT, "default_language_in_use".to_string());
    // Alone, and swapped with a language on the list.
    assert_eq!(patch(r#"{"default_language":"es"}"#).await, in_use);
    assert_eq!(
        patch(
            r#"{"default_language":"de","languages":[{"language":"en","published":true},{"language":"de-AT"},{"language":"fr"}]}"#
        )
        .await,
        in_use
    );
    // The same default again is no change.
    assert_eq!(
        patch(r#"{"default_language":"en"}"#).await.0,
        StatusCode::OK
    );

    // Translations into a language the same request takes off the list don't count:
    // they go with it.
    assert_eq!(
        patch(r#"{"default_language":"es","languages":[{"language":"de"}]}"#)
            .await
            .0,
        StatusCode::OK
    );
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let (_, page) = send(&app, "GET", "/admin/translations", None).await;
    assert_eq!(page["items"], json!([]));
}
