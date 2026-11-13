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
    assert_eq!(title["min_length"], Value::Null);
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
    // Nothing to translate where the product has no description: refused, not kept
    // unseen.
    let body = json!({"fields": {"description": {"value": "<p>Eine Tasse.</p>"}}}).to_string();
    let (status, refused) = translate(&app, &path, "de", &body).await;
    assert_eq!(
        (status, code(&refused)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_translation")
    );
    assert_eq!(problems(&refused), [("description", "source_empty")]);
    let (status, json) = send(
        &app,
        "PATCH",
        &format!("/admin/products/{product}"),
        Some(r#"{"description":"<p>A mug.</p>"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");

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

    // A second option, removed from its zone on its own below.
    let express = json!({
        "name": "Express", "code": "express",
        "rate": {"type": "builtin", "basis": "subtotal", "tables": [
            {"currency": "EUR", "tiers": [{"amount": 990}]}
        ]},
    });
    let zone = created(&format!("{zone_path}/options"), express).await;
    let express = zone["options"][1]["id"].as_str().unwrap().to_string();
    translated("shipping_option", &express, "name", "Express").await;

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
            "shipping_option",
            "store"
        ]
    );

    // What's deleted takes its translations along once its event is delivered: an
    // option removed from its zone, a zone with its options, a payment method, …
    let (status, body) = send(
        &app,
        "DELETE",
        &format!("{zone_path}/options/{express}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    assert_eq!(
        listed()
            .await
            .iter()
            .filter(|k| *k == "shipping_option")
            .count(),
        1,
        "the removed option's went"
    );
    for uri in [
        format!("/admin/payment-methods/{}", id(&method)),
        zone_path.clone(),
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
    let (status, json) = send(
        &app,
        "PATCH",
        "/admin/store",
        Some(r#"{"description":"Mugs"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let path = format!("/admin/translations/store/{store}");
    let (status, _) = translate(&app, &path, "de", &fields(&[("description", "Tassen")])).await;
    assert_eq!(status, StatusCode::OK);
    hypha_http::jobs::dispatch(&app.state).await.unwrap();

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
            // The description has text, the emails' footer none.
            "state": "complete",
            "total": 1,
            "missing": 0,
            "outdated": 0,
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

/// A swap makes a listed language the default, and the cleanup keeps only listed ones:
/// translations into the new default count, or they'd all be deleted.
#[tokio::test]
async fn the_default_language_doesnt_swap_with_a_translated_one() {
    let app = app().await;
    let body = r#"{"languages":[{"language":"de","published":true},{"language":"fr"}]}"#;
    let (status, store) = send(&app, "PATCH", "/admin/store", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{store}");
    let product = create_product(&app, "mug").await;
    let path = format!("/admin/translations/product/{product}");
    let (status, _) = translate(&app, &path, "de", &fields(&[("title", "Tasse")])).await;
    assert_eq!(status, StatusCode::OK);

    let swap = r#"{"default_language":"de","languages":[{"language":"en","published":true}]}"#;
    let (status, json) = send(&app, "PATCH", "/admin/store", Some(swap)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "default_language_in_use")
    );
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let (_, view) = send(&app, "GET", &path, None).await;
    assert_eq!(view["translations"][0]["language"], "de");
    assert_eq!(
        field(&view, "title")["translations"]["de"]["value"],
        "Tasse"
    );

    // Once the German is gone with its language, the same swap goes through, dropping
    // French, which has no translations either.
    let body = r#"{"languages":[{"language":"fr"}]}"#;
    let (status, _) = send(&app, "PATCH", "/admin/store", Some(body)).await;
    assert_eq!(status, StatusCode::OK);
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let (status, store) = send(&app, "PATCH", "/admin/store", Some(swap)).await;
    assert_eq!(status, StatusCode::OK, "{store}");
    assert_eq!(store["default_language"], "de");
}

/// A page of the worklist: each item's resource ID, state and counts.
async fn worklist(app: &TestApp, query: &str) -> Value {
    let (status, page) = send(app, "GET", &format!("/admin/translations?{query}"), None).await;
    assert_eq!(status, StatusCode::OK, "{query}: {page}");
    page
}

fn ids(page: &Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["resource"]["id"].as_str().unwrap())
        .collect()
}

fn kinds(page: &Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["resource"]["kind"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn the_worklist_lists_each_resource_by_its_state_in_a_language() {
    let app = app().await;
    in_four_languages(&app).await;
    // UUIDv7: created in this order, listed in it.
    let mug = create_product(&app, "mug").await;
    let plate = create_product(&app, "plate").await;
    let cup = create_product(&app, "cup").await;
    for product in [&mug, &cup] {
        let path = format!("/admin/translations/product/{product}");
        let (status, body) = translate(&app, &path, "de", &fields(&[("title", "X")])).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    hypha_http::jobs::dispatch(&app.state).await.unwrap();

    // Only the title has text: the empty description counts for nothing.
    let complete = worklist(&app, "kind=product&language=de&state=complete").await;
    assert_eq!(ids(&complete), [mug.as_str(), cup.as_str()]);
    let item = &complete["items"][0];
    assert_eq!(
        (
            &item["state"],
            &item["total"],
            &item["missing"],
            &item["outdated"]
        ),
        (&json!("complete"), &json!(1), &json!(0), &json!(0))
    );
    assert_eq!(item["language"], "de");
    assert_eq!(item["fields"], json!(["title"]));
    assert!(item["created_at"].is_string());

    let missing = worklist(&app, "kind=product&language=de&state=missing").await;
    assert_eq!(ids(&missing), [plate.as_str()]);
    // A resource without a translation in the language is on it all the same.
    assert_eq!(
        (
            &missing["items"][0]["fields"],
            &missing["items"][0]["created_at"]
        ),
        (&json!([]), &Value::Null)
    );

    // The cup's title changes: its translation is outdated once the event is delivered.
    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/admin/products/{cup}"),
        Some(r#"{"title":"Cup"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        ids(&worklist(&app, "kind=product&language=de&state=outdated").await),
        Vec::<&str>::new(),
        "not yet delivered"
    );
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let outdated = worklist(&app, "kind=product&language=de&state=outdated").await;
    assert_eq!(ids(&outdated), [cup.as_str()]);
    assert_eq!(
        (
            &outdated["items"][0]["missing"],
            &outdated["items"][0]["outdated"]
        ),
        (&json!(0), &json!(1))
    );
    // A description makes the mug's translation miss a field again.
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/admin/products/{mug}"),
        Some(r#"{"description":"<p>A mug.</p>"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let missing = worklist(&app, "kind=product&language=de&state=missing").await;
    assert_eq!(ids(&missing), [mug.as_str(), plate.as_str()]);
    assert_eq!(
        (
            &missing["items"][0]["total"],
            &missing["items"][0]["missing"]
        ),
        (&json!(2), &json!(1))
    );

    // Paged, each page continuing the one before.
    let first = worklist(&app, "kind=product&language=fr&state=missing&limit=2").await;
    assert_eq!(ids(&first), [mug.as_str(), plate.as_str()]);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = worklist(
        &app,
        &format!("kind=product&language=fr&state=missing&limit=2&cursor={cursor}"),
    )
    .await;
    assert_eq!(ids(&second), [cup.as_str()]);
    assert_eq!(second["has_more"], false);

    // Kinds in the catalog's order: products, then categories, not by name.
    let (status, _) = send(
        &app,
        "POST",
        "/admin/categories",
        Some(r#"{"name":"Mugs","handle":"mugs","visible":true}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let all = worklist(&app, "language=fr&state=missing&limit=100").await;
    assert_eq!(
        kinds(&all)[..4],
        ["product", "product", "product", "category"]
    );

    // The translations there are, each with its state now.
    let listed = worklist(&app, "kind=product&language=de").await;
    let states: Vec<&Value> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| &i["state"])
        .collect();
    assert_eq!(states, [&json!("missing"), &json!("outdated")]);

    let plain = worklist(&app, "limit=1").await;
    let cursor = plain["next_cursor"].as_str().unwrap().to_string();
    for (query, status, expected) in [
        ("state=missing", StatusCode::BAD_REQUEST, "invalid_query"),
        (
            "language=en&state=missing",
            StatusCode::UNPROCESSABLE_ENTITY,
            "translation_into_default_language",
        ),
        (
            "language=es&state=missing",
            StatusCode::UNPROCESSABLE_ENTITY,
            "language_not_listed",
        ),
        (
            "language=de&state=done",
            StatusCode::BAD_REQUEST,
            "invalid_query",
        ),
        (
            &*format!("language=de&state=missing&cursor={cursor}"),
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
        ),
    ] {
        let (got, body) = send(&app, "GET", &format!("/admin/translations?{query}"), None).await;
        assert_eq!((got, code(&body)), (status, expected), "{query}");
    }
}

#[tokio::test]
async fn progress_counts_each_languages_fields() {
    let app = app().await;
    in_four_languages(&app).await;
    let mug = create_product(&app, "mug").await;
    let plate = create_product(&app, "plate").await;
    for (product, language) in [(&mug, "de"), (&plate, "de"), (&mug, "fr")] {
        let path = format!("/admin/translations/product/{product}");
        let (status, _) = translate(&app, &path, language, &fields(&[("title", "X")])).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/admin/products/{plate}"),
        Some(r#"{"title":"Plate","description":"<p>A plate.</p>"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    hypha_http::jobs::dispatch(&app.state).await.unwrap();

    let (status, progress) = send(
        &app,
        "GET",
        "/admin/translations/progress?kind=product",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{progress}");
    // Three fields with text: the mug's title, the plate's title and description.
    assert_eq!(
        progress,
        json!({
            "default_language": "en",
            "languages": [
                {"language": "de", "total": 3, "translated": 2, "outdated": 1, "missing": 1},
                {"language": "de-AT", "total": 3, "translated": 0, "outdated": 0, "missing": 3},
                {"language": "fr", "total": 3, "translated": 1, "outdated": 0, "missing": 2},
            ],
            "pending": 0,
        })
    );
    let (status, all) = send(&app, "GET", "/admin/translations/progress", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all["languages"].as_array().unwrap().len(), 3);

    // Only the kinds the caller reads.
    let reader = key(
        &app,
        json!({"kind": "secret", "name": "Reader", "scopes": ["translations:read"]}),
    )
    .await;
    let (status, body) = send_as(
        &app,
        &reader,
        "GET",
        "/admin/translations/progress?kind=product",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
    let (status, body) = send_as(&app, &reader, "GET", "/admin/translations/progress", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["languages"][0]["total"], 0);
    let (status, body) = send(
        &app,
        "GET",
        "/admin/translations/progress?kind=variant",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_query")
    );
}

#[tokio::test]
async fn progress_says_how_many_resources_are_still_being_counted() {
    let app = app().await;
    in_four_languages(&app).await;
    create_product(&app, "mug").await;
    create_product(&app, "plate").await;
    let progress = async |query: &str| {
        let (status, body) = send(
            &app,
            "GET",
            &format!("/admin/translations/progress{query}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    };
    // Their events not delivered, then queued as the migration queues what was there
    // before the worklist: nothing counted yet, and the count says so.
    sql(
        &app,
        "INSERT INTO translation_source_queue (resource_kind, resource_id)
                   SELECT 'product', id FROM products
         UNION ALL SELECT 'store', id FROM stores",
    )
    .await;
    let before = progress("").await;
    assert_eq!(before["pending"], 3, "{before}");
    assert_eq!(before["languages"][0]["total"], 0);
    let products = progress("?kind=product").await;
    assert_eq!(
        products["pending"], 2,
        "only the kind asked for: {products}"
    );

    loop {
        let ran = hypha_http::jobs::translation_sources(&app.state)
            .await
            .unwrap();
        if ran.idle {
            break;
        }
    }
    let after = progress("?kind=product").await;
    assert_eq!(after["pending"], 0, "{after}");
    assert_eq!(
        after["languages"][0]["total"], 2,
        "the mug's and the plate's titles"
    );
}

#[tokio::test]
async fn every_kinds_text_is_followed_by_event() {
    let app = app_with_media().await;
    in_four_languages(&app).await;
    let created = async |uri: &str, body: Value| {
        let (status, json) = send(&app, "POST", uri, Some(&body.to_string())).await;
        assert_eq!(status, StatusCode::CREATED, "{uri}: {json}");
        json
    };
    let id = |json: &Value| json["id"].as_str().unwrap().to_string();
    let counts = async |kind: &str, id: &str| {
        let page = worklist(
            &app,
            &format!("kind={kind}&language=de&state=missing&limit=100"),
        )
        .await;
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["resource"]["id"] == id)
            .map(|i| i["total"].as_u64().unwrap())
    };

    let method = created(
        "/admin/payment-methods",
        json!({"provider": "manual", "name": "Cash on delivery"}),
    )
    .await;
    let method = id(&method);
    let zone = created(
        "/admin/shipping-zones",
        json!({"name": "Croatia", "countries": ["HR"]}),
    )
    .await;
    let zone_path = format!("/admin/shipping-zones/{}", id(&zone));
    let option = json!({
        "name": "Standard", "code": "standard",
        "rate": {"type": "builtin", "basis": "subtotal", "tables": [
            {"currency": "EUR", "tiers": [{"amount": 490}]}
        ]},
    });
    let zone = created(&format!("{zone_path}/options"), option).await;
    let option = zone["options"][0]["id"].as_str().unwrap().to_string();
    let asset = ready_png(&app).await;
    let store = store_id(&app).await;
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    assert_eq!(counts("payment_method", &method).await, Some(1));
    assert_eq!(counts("shipping_option", &option).await, Some(1));
    // No alt, no description, no footer, no facets yet: nothing to translate.
    assert_eq!(counts("asset", &asset).await, None);
    assert_eq!(counts("store", &store).await, None);
    assert_eq!(counts("search_facet", &store).await, None);

    // Each change is followed: a method's instructions, an asset's alt, the store's
    // description and its emails' footer, the facets' labels.
    let changes = [
        (
            "PATCH",
            format!("/admin/payment-methods/{method}"),
            json!({"instructions": "Pay the courier."}),
        ),
        (
            "PATCH",
            format!("/admin/assets/{asset}"),
            json!({"alt": "A mug"}),
        ),
        (
            "PATCH",
            "/admin/store".to_string(),
            json!({"description": "Mugs."}),
        ),
        (
            "PATCH",
            "/admin/notification-settings".to_string(),
            json!({"footer": "Ilica 1, Zagreb"}),
        ),
        (
            "PATCH",
            "/admin/search/settings".to_string(),
            json!({"facets": [{"key": "vendor", "label": "Maker"}]}),
        ),
    ];
    for (method, uri, body) in changes {
        let (status, json) = send(&app, method, &uri, Some(&body.to_string())).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {json}");
    }
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    assert_eq!(counts("payment_method", &method).await, Some(2));
    assert_eq!(counts("asset", &asset).await, Some(1));
    assert_eq!(counts("store", &store).await, Some(2));
    assert_eq!(counts("search_facet", &store).await, Some(1));

    // What's deleted leaves the worklist with its event.
    for uri in [format!("/admin/payment-methods/{method}"), zone_path] {
        let (status, body) = send(&app, "DELETE", &uri, None).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{uri}: {body}");
    }
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    assert_eq!(counts("payment_method", &method).await, None);
    assert_eq!(counts("shipping_option", &option).await, None);
}

#[tokio::test]
async fn resources_queued_as_a_whole_are_read_again_by_the_job() {
    let app = app().await;
    in_four_languages(&app).await;
    let mug = create_product(&app, "mug").await;
    // Its events not delivered: as a product from before the worklist was.
    let missing =
        async || ids(&worklist(&app, "kind=product&language=de&state=missing").await).len();
    assert_eq!(missing().await, 0);
    // The plain list reads a resource without digests yet itself: its state isn't a
    // guess.
    let path = format!("/admin/translations/product/{mug}");
    let (status, _) = translate(&app, &path, "fr", &fields(&[("title", "X")])).await;
    assert_eq!(status, StatusCode::OK);
    let listed = worklist(&app, "kind=product&language=fr").await;
    assert_eq!(
        (&listed["items"][0]["state"], &listed["items"][0]["total"]),
        (&json!("complete"), &json!(1))
    );
    let uuid = mug.trim_start_matches("prod_");
    sql(
        &app,
        &format!(
            "INSERT INTO translation_source_queue (resource_kind, resource_id)
             VALUES ('product', '{uuid}')"
        ),
    )
    .await;
    let ran = hypha_http::jobs::translation_sources(&app.state)
        .await
        .unwrap();
    assert!(!ran.idle && !ran.more, "{ran:?}");
    assert_eq!(missing().await, 1);
    let ran = hypha_http::jobs::translation_sources(&app.state)
        .await
        .unwrap();
    assert!(ran.idle, "the queue is empty: {ran:?}");

    // A definition whose values were translated goes: every product is read again.
    let (status, definition) = send(
        &app,
        "POST",
        "/admin/custom-field-definitions",
        Some(
            &json!({"owner_type": "product", "key": "material", "name": "Material",
                    "type": "text", "access": {"admin": "read_write", "storefront": "read"}})
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{definition}");
    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/admin/products/{mug}/custom-fields"),
        Some(r#"{"custom_fields":{"custom.material":"Stoneware"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let total = async || {
        worklist(&app, "kind=product&language=de&state=missing").await["items"][0]["total"].clone()
    };
    assert_eq!(total().await, 2, "the title and the material");
    let definition = definition["id"].as_str().unwrap();
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/admin/custom-field-definitions/{definition}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    hypha_http::jobs::translation_sources(&app.state)
        .await
        .unwrap();
    assert_eq!(total().await, 1);
}
