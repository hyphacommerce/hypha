//! Custom field values in other languages: which ones are translatable, how a product's
//! and a category's translations hold them, what storefronts read and what removes them.

use serde_json::{Value, json};

use super::*;

/// German beside English, published, and offered by the mug's market.
async fn in_german(app: &TestApp) -> (String, String) {
    let body = r#"{"languages":[{"language":"de","published":true}]}"#;
    let (status, store) = send(app, "PATCH", "/admin/store", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{store}");
    let (market, variant) = market_selling_a_mug(app).await;
    let body = json!({ "languages": ["en", "de"] }).to_string();
    let (status, json) = send(
        app,
        "PATCH",
        &format!("/admin/markets/{market}"),
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (_, mugs) = send(app, "GET", "/admin/products?limit=1", None).await;
    (
        mugs["items"][0]["id"].as_str().unwrap().to_string(),
        variant,
    )
}

/// A `custom` definition storefronts read, with `extra` merged in; returns its ID.
async fn define(
    app: &TestApp,
    owner_type: &str,
    key: &str,
    field_type: &str,
    extra: Value,
) -> String {
    let mut body = json!({
        "owner_type": owner_type, "key": key, "name": key.to_uppercase(), "type": field_type,
        "access": {"admin": "read_write", "storefront": "read"},
    });
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    let uri = "/admin/custom-field-definitions";
    let (status, json) = send(app, "POST", uri, Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    json["id"].as_str().unwrap().to_string()
}

async fn set_fields(app: &TestApp, path: &str, values: Value) {
    let body = json!({ "custom_fields": values }).to_string();
    let (status, json) = send(app, "PATCH", &format!("{path}/custom-fields"), Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

fn body(fields: &[(&str, &str)]) -> String {
    let fields: serde_json::Map<String, Value> = fields
        .iter()
        .map(|(key, value)| (key.to_string(), json!({ "value": value })))
        .collect();
    json!({ "fields": fields }).to_string()
}

fn keys(view: &Value) -> Vec<&str> {
    view["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap())
        .collect()
}

fn field<'a>(view: &'a Value, key: &str) -> &'a Value {
    view["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == key)
        .unwrap_or_else(|| panic!("no field {key} in {view}"))
}

fn problems(body: &Value) -> Vec<(&str, &str)> {
    body["details"]["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["key"].as_str().unwrap(), p["code"].as_str().unwrap()))
        .collect()
}

async fn storefront_mug(app: &TestApp, pk: &str, query: &str) -> Value {
    let uri = format!("/storefront/products/by-handle/mug?country=HR{query}");
    let (status, json) = send_as(app, pk, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    json
}

#[tokio::test]
async fn a_products_text_values_are_translated_and_read_in_german() {
    let app = app().await;
    let (mug, variant) = in_german(&app).await;
    let care = define(&app, "product", "care", "text", json!({})).await;
    let story = define(&app, "product", "story", "rich_text", json!({})).await;
    let fit = define(
        &app,
        "variant",
        "fit",
        "multiline_text",
        json!({"validations": {"max_length": 20}}),
    )
    .await;
    // Not translatable: a code (a pattern), a field storefronts aren't shown, a choice
    // (its labels are the definition's) and a list.
    define(
        &app,
        "product",
        "code",
        "text",
        json!({"validations": {"pattern": "^[A-Z]+$"}}),
    )
    .await;
    define(
        &app,
        "product",
        "note",
        "text",
        json!({"access": {"admin": "read_write", "storefront": "none"}}),
    )
    .await;
    define(&app, "product", "tags", "list.text", json!({})).await;
    let product = format!("/admin/products/{mug}");
    set_fields(
        &app,
        &product,
        json!({
            "custom.care": "Hand wash", "custom.story": "<p>Thrown by hand.</p>",
            "custom.code": "MUG", "custom.note": "Reorder in May", "custom.tags": ["clay"],
        }),
    )
    .await;
    set_fields(
        &app,
        &format!("/admin/variants/{variant}"),
        json!({"custom.fit": "Loose"}),
    )
    .await;

    // The editor's view: the product's fields, then its values and its variant's, each
    // named by its definition.
    let path = format!("/admin/translations/product/{mug}");
    let (status, view) = send(&app, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let care_key = format!("custom_fields.{care}.value");
    let story_key = format!("custom_fields.{story}.value");
    let fit_key = format!("variant_custom_fields.{variant}.{fit}.value");
    assert_eq!(
        keys(&view)[2..],
        [care_key.as_str(), story_key.as_str(), fit_key.as_str()],
        "{view}"
    );
    let care_field = field(&view, &care_key);
    assert_eq!(
        (
            &care_field["source"],
            &care_field["format"],
            &care_field["max_length"]
        ),
        (&json!("Hand wash"), &json!("text"), &json!(255))
    );
    assert_eq!(
        care_field["custom_field"],
        json!({
            "definition": care, "key": "custom.care", "name": "CARE", "type": "text",
            "variant": null, "variant_title": null,
        })
    );
    assert_eq!(field(&view, &story_key)["format"], "html");
    let fit_field = field(&view, &fit_key);
    assert_eq!(fit_field["max_length"], 20);
    assert_eq!(fit_field["custom_field"]["variant"], json!(variant));
    assert_eq!(field(&view, "title")["custom_field"], Value::Null);

    // Each value obeys its definition: one line for `text`, the definition's length.
    let refused = body(&[(&care_key, "Hand\nwäsche"), (&fit_key, &"l".repeat(21))]);
    let (status, json) = send(&app, "PUT", &format!("{path}/de"), Some(&refused)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_translation")
    );
    let mut found = problems(&json);
    found.sort_unstable();
    let mut expected = vec![
        (care_key.as_str(), "not_single_line"),
        (fit_key.as_str(), "too_long"),
    ];
    expected.sort_unstable();
    assert_eq!(found, expected);
    let translated = body(&[
        ("title", "Tasse"),
        (&care_key, "Handwäsche"),
        (&story_key, "<p>Von Hand gedreht.</p><script>x</script>"),
        (&fit_key, "Locker\ngeschnitten"),
    ]);
    let (status, json) = send(&app, "PUT", &format!("{path}/de"), Some(&translated)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        field(&json, &story_key)["translations"]["de"]["value"],
        "<p>Von Hand gedreht.</p>"
    );

    // Storefronts read them in German, the rest as stored; the source in English.
    let pk = publishable(&app).await;
    let german = storefront_mug(&app, &pk, "&language=de").await;
    assert_eq!(german["title"], "Tasse");
    assert_eq!(
        german["custom_fields"],
        json!({
            "custom.care": "Handwäsche", "custom.story": "<p>Von Hand gedreht.</p>",
            "custom.code": "MUG", "custom.tags": ["clay"],
        })
    );
    assert_eq!(
        german["variants"][0]["custom_fields"],
        json!({"custom.fit": "Locker\ngeschnitten"})
    );
    let english = storefront_mug(&app, &pk, "").await;
    assert_eq!(english["custom_fields"]["custom.care"], "Hand wash");
    assert_eq!(
        english["variants"][0]["custom_fields"]["custom.fit"],
        "Loose"
    );
    // Admin reads answer the source.
    let (_, admin) = send(&app, "GET", &product, None).await;
    assert_eq!(admin["custom_fields"]["custom.care"], "Hand wash");
}

#[tokio::test]
async fn a_categorys_text_value_is_read_in_german() {
    let app = app().await;
    let body_de = r#"{"languages":[{"language":"de","published":true}]}"#;
    send(&app, "PATCH", "/admin/store", Some(body_de)).await;
    let season = define(&app, "category", "season", "text", json!({})).await;
    let new = json!({"name": "Mugs", "handle": "mugs", "visible": true}).to_string();
    let (status, category) = send(&app, "POST", "/admin/categories", Some(&new)).await;
    assert_eq!(status, StatusCode::CREATED, "{category}");
    let id = category["id"].as_str().unwrap();
    set_fields(
        &app,
        &format!("/admin/categories/{id}"),
        json!({"custom.season": "Winter"}),
    )
    .await;
    let key = format!("custom_fields.{season}.value");
    let path = format!("/admin/translations/category/{id}/de");
    let (status, json) = send(&app, "PUT", &path, Some(&body(&[(&key, "Wintersaison")]))).await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let pk = publishable(&app).await;
    let uri = "/storefront/categories/by-handle/mugs";
    let (_, german) = send_as(&app, &pk, "GET", &format!("{uri}?language=de"), None).await;
    assert_eq!(
        german["custom_fields"],
        json!({"custom.season": "Wintersaison"})
    );
    let (_, english) = send_as(&app, &pk, "GET", uri, None).await;
    assert_eq!(english["custom_fields"], json!({"custom.season": "Winter"}));
}

#[tokio::test]
async fn customers_and_orders_values_are_never_translated() {
    let app = app().await;
    let body = json!({ "email": "ana@example.hr" }).to_string();
    let (_, customer) = send(&app, "POST", "/admin/customers", Some(&body)).await;
    let id = customer["id"].as_str().unwrap();
    for path in [
        format!("/admin/translations/customer/{id}"),
        "/admin/translations/order/ord_00000000000000000000000000000001".to_string(),
        "/admin/translations/custom_field_value/cfv_00000000000000000000000000000001".to_string(),
    ] {
        let (status, json) = send(&app, "GET", &path, None).await;
        assert_eq!(
            (status, code(&json)),
            (StatusCode::NOT_FOUND, "unknown_resource_kind"),
            "{path}"
        );
    }
}

/// A definition that stops qualifying (here, gaining a pattern; equally made private
/// or hidden from storefronts) leaves the editor and storefronts, and its stored
/// translation drops out on the language's next write.
#[tokio::test]
async fn a_value_that_stops_qualifying_isnt_read_and_drops_out_on_the_next_write() {
    let app = app().await;
    let (mug, _) = in_german(&app).await;
    let care = define(&app, "product", "care", "text", json!({})).await;
    set_fields(
        &app,
        &format!("/admin/products/{mug}"),
        json!({"custom.care": "HAND"}),
    )
    .await;
    let key = format!("custom_fields.{care}.value");
    let path = format!("/admin/translations/product/{mug}");
    let both = body(&[("title", "Tasse"), (&key, "HANDWAESCHE")]);
    let (status, json) = send(&app, "PUT", &format!("{path}/de"), Some(&both)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let pk = publishable(&app).await;
    let german = storefront_mug(&app, &pk, "&language=de").await;
    assert_eq!(german["custom_fields"]["custom.care"], "HANDWAESCHE");
    // The values (and the translation) match the pattern: it's taken.
    let body_pattern = json!({"validations": {"pattern": "^[A-Z]+$"}}).to_string();
    let uri = format!("/admin/custom-field-definitions/{care}");
    let (status, json) = send(&app, "PATCH", &uri, Some(&body_pattern)).await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let (_, view) = send(&app, "GET", &path, None).await;
    assert!(!keys(&view).contains(&key.as_str()), "{view}");
    let german = storefront_mug(&app, &pk, "&language=de").await;
    assert_eq!(german["custom_fields"]["custom.care"], "HAND");
    // Still stored, until the language is written again.
    let (_, listed) = send(&app, "GET", "/admin/translations?kind=product", None).await;
    assert_eq!(listed["items"][0]["fields"], json!([key, "title"]));
    let (status, json) = send(&app, "PUT", &format!("{path}/de"), Some(&both)).await;
    assert_eq!(
        (status, problems(&json)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            vec![(key.as_str(), "unknown_child")]
        )
    );
    let title = body(&[("title", "Tasse")]);
    let (status, json) = send(&app, "PUT", &format!("{path}/de"), Some(&title)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (_, listed) = send(&app, "GET", "/admin/translations?kind=product", None).await;
    assert_eq!(listed["items"][0]["fields"], json!(["title"]));
}

#[tokio::test]
async fn a_removed_value_and_a_deleted_owner_take_their_translations() {
    let app = app().await;
    let (mug, _) = in_german(&app).await;
    let care = define(&app, "product", "care", "text", json!({})).await;
    let product = format!("/admin/products/{mug}");
    set_fields(&app, &product, json!({"custom.care": "Hand wash"})).await;
    let key = format!("custom_fields.{care}.value");
    let path = format!("/admin/translations/product/{mug}");
    let both = body(&[("title", "Tasse"), (&key, "Handwäsche")]);
    let (status, json) = send(&app, "PUT", &format!("{path}/de"), Some(&both)).await;
    assert_eq!(status, StatusCode::OK, "{json}");

    // Removed, then set again: the new value isn't shown the old one's translation.
    set_fields(&app, &product, json!({"custom.care": null})).await;
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    set_fields(&app, &product, json!({"custom.care": "Dishwasher safe"})).await;
    let (_, listed) = send(&app, "GET", "/admin/translations?kind=product", None).await;
    assert_eq!(listed["items"][0]["fields"], json!(["title"]));
    let pk = publishable(&app).await;
    let german = storefront_mug(&app, &pk, "&language=de").await;
    assert_eq!(german["custom_fields"]["custom.care"], "Dishwasher safe");
    // Set again to the same text before the cleanup ran: the translation is still that
    // text's, and stays.
    let again = body(&[("title", "Tasse"), (&key, "Spülmaschinenfest")]);
    send(&app, "PUT", &format!("{path}/de"), Some(&again)).await;
    set_fields(&app, &product, json!({"custom.care": null})).await;
    set_fields(&app, &product, json!({"custom.care": "Dishwasher safe"})).await;
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let german = storefront_mug(&app, &pk, "&language=de").await;
    assert_eq!(german["custom_fields"]["custom.care"], "Spülmaschinenfest");
    // Set again to other text before the cleanup ran: the removed value's translation
    // goes all the same.
    set_fields(&app, &product, json!({"custom.care": null})).await;
    set_fields(&app, &product, json!({"custom.care": "Oven safe"})).await;
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let german = storefront_mug(&app, &pk, "&language=de").await;
    assert_eq!(german["custom_fields"]["custom.care"], "Oven safe");
    // A translation of the new value, written before the cleanup ran, stays.
    let oven = body(&[("title", "Tasse"), (&key, "Ofenfest")]);
    set_fields(&app, &product, json!({"custom.care": null})).await;
    set_fields(&app, &product, json!({"custom.care": "Oven proof"})).await;
    let (status, json) = send(&app, "PUT", &format!("{path}/de"), Some(&oven)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let german = storefront_mug(&app, &pk, "&language=de").await;
    assert_eq!(german["custom_fields"]["custom.care"], "Ofenfest");

    // The product deleted: its translation goes, values and all.
    let (status, _) = send(&app, "DELETE", &product, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let (_, listed) = send(&app, "GET", "/admin/translations?kind=product", None).await;
    assert_eq!(listed["items"], json!([]));
}
