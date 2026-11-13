//! Storefront reads in a language: which one a request gets, what's translated in the
//! response, and what stays in the store's own.

use axum::http::header::{AUTHORIZATION, CONTENT_LANGUAGE};
use serde_json::{Value, json};

use super::*;

/// The store's other languages: `[(tag, published)]` beside English.
async fn languages(app: &TestApp, languages: &[(&str, bool)]) {
    let languages: Vec<Value> = languages
        .iter()
        .map(|(language, published)| json!({"language": language, "published": published}))
        .collect();
    let body = json!({ "languages": languages }).to_string();
    let (status, store) = send(app, "PATCH", "/admin/store", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{store}");
}

/// The languages the first market offers, in order.
async fn market_offers(app: &TestApp, languages: &[&str]) {
    let (_, markets) = send(app, "GET", "/admin/markets", None).await;
    let market = markets["items"][0]["id"].as_str().unwrap();
    let body = json!({ "languages": languages }).to_string();
    let path = format!("/admin/markets/{market}");
    let (status, json) = send(app, "PATCH", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

/// Translates `{kind}/{id}` into `language`: `[(field key, value)]`.
async fn translate(app: &TestApp, kind: &str, id: &str, language: &str, fields: &[(&str, &str)]) {
    let fields: serde_json::Map<String, Value> = fields
        .iter()
        .map(|(key, value)| (key.to_string(), json!({ "value": value })))
        .collect();
    let path = format!("/admin/translations/{kind}/{id}/{language}");
    let body = json!({ "fields": fields }).to_string();
    let (status, json) = send(app, "PUT", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{kind} {language}: {json}");
}

/// A storefront's read: the status, the response's `Content-Language` and its body.
async fn read(app: &TestApp, key: &str, uri: &str) -> (StatusCode, Option<String>, Value) {
    let request = Request::builder()
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {key}"))
        .body(Body::empty())
        .unwrap();
    let response = app.router().oneshot(request).await.unwrap();
    let status = response.status();
    let language = response
        .headers()
        .get(CONTENT_LANGUAGE)
        .map(|l| l.to_str().unwrap().to_string());
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        language,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A successful read: the language it was answered in and its body.
async fn read_ok(app: &TestApp, key: &str, uri: &str) -> (String, Value) {
    let (status, language, body) = read(app, key, uri).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    (
        language.unwrap_or_else(|| panic!("{uri}: no Content-Language")),
        body,
    )
}

/// The mug of `market_selling_a_mug`, described, in two sizes and one colour. Returns
/// (its ID, the option keys of Size, S and Red).
async fn sized_mug(app: &TestApp) -> (String, [String; 3]) {
    market_selling_a_mug(app).await;
    let (_, mugs) = send(app, "GET", "/admin/products?limit=1", None).await;
    let mug = mugs["items"][0]["id"].as_str().unwrap().to_string();
    let path = format!("/admin/products/{mug}");
    let (status, json) = send(
        app,
        "PATCH",
        &path,
        Some(r#"{"description":"<p>For coffee.</p>"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let options = r#"{"options":[
        {"name":"Size","values":[{"label":"S"},{"label":"M"}]},
        {"name":"Color","values":[{"label":"Red"}]}
    ]}"#;
    let (status, sized) = send(app, "PUT", &format!("{path}/options"), Some(options)).await;
    assert_eq!(status, StatusCode::OK, "{sized}");
    let id = |value: &Value| value["id"].as_str().unwrap().to_string();
    let (size, color) = (&sized["options"][0], &sized["options"][1]);
    let body = json!({
        "price": {"amount": 1200, "currency": "EUR"},
        "selections": [size["values"][1]["id"], color["values"][0]["id"]],
    });
    let (status, json) = send(
        app,
        "POST",
        &format!("{path}/variants"),
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    let keys = [
        format!("options.{}.name", id(size)),
        format!("option_values.{}.label", id(&size["values"][0])),
        format!("option_values.{}.label", id(&color["values"][0])),
    ];
    (mug, keys)
}

#[tokio::test]
async fn a_product_is_read_in_the_language_asked_for() {
    let app = app().await;
    languages(&app, &[("de", true), ("de-AT", true), ("fr", false)]).await;
    let (mug, [size, small, red]) = sized_mug(&app).await;
    market_offers(&app, &["en", "de", "de-AT"]).await;
    // German: everything but the description, the second option's name and "M".
    translate(
        &app,
        "product",
        &mug,
        "de",
        &[
            ("title", "Tasse"),
            (&size, "Größe"),
            (&small, "Klein"),
            (&red, "Rot"),
        ],
    )
    .await;
    // Austrian German: only what differs.
    translate(&app, "product", &mug, "de-AT", &[("title", "Häferl")]).await;
    translate(&app, "product", &mug, "fr", &[("title", "Tasse à café")]).await;
    let pk = publishable(&app).await;
    let by_handle = "/storefront/products/by-handle/mug?country=HR";

    let (language, product) = read_ok(&app, &pk, &format!("{by_handle}&language=de")).await;
    assert_eq!(language, "de");
    assert_eq!(product["title"], "Tasse");
    assert_eq!(
        product["description"], "<p>For coffee.</p>",
        "a field without a translation stays in the store's language"
    );
    assert_eq!(product["handle"], "mug");
    let options = &product["options"];
    assert_eq!(
        (&options[0]["name"], &options[1]["name"]),
        (&json!("Größe"), &json!("Color"))
    );
    assert_eq!(
        (
            &options[0]["values"][0]["label"],
            &options[0]["values"][1]["label"]
        ),
        (&json!("Klein"), &json!("M"))
    );
    assert_eq!(options[1]["values"][0]["label"], "Rot");
    assert_eq!(
        (
            &product["variants"][0]["title"],
            &product["variants"][1]["title"]
        ),
        (&json!("Klein / Rot"), &json!("M / Rot")),
        "joined from the translated labels"
    );
    assert_eq!(product["variants"][0]["price"]["amount"], 1000);

    // The read by ID is the same read.
    let by_id = format!("/storefront/products/{mug}?country=HR&language=de");
    assert_eq!(read_ok(&app, &pk, &by_id).await, (language, product));

    // A regional wording falls back to its language, field by field.
    let (language, product) = read_ok(&app, &pk, &format!("{by_handle}&language=de-at")).await;
    assert_eq!(language, "de-AT", "in the API's spelling");
    assert_eq!(product["title"], "Häferl");
    assert_eq!(product["options"][0]["name"], "Größe");
    assert_eq!(product["variants"][0]["title"], "Klein / Rot");

    // No language, the store's own, one it hasn't published, and nonsense: the
    // market's default, never an error.
    for query in [
        "",
        "&language=en",
        "&language=fr",
        "&language=german",
        "&language=",
    ] {
        let (language, product) = read_ok(&app, &pk, &format!("{by_handle}{query}")).await;
        assert_eq!(language, "en", "{query:?}");
        assert_eq!(product["title"], "Mug", "{query:?}");
        assert_eq!(product["variants"][0]["title"], "S / Red", "{query:?}");
    }

    // The admin reads the source, whatever it's translated into.
    let (_, admin) = send(&app, "GET", &format!("/admin/products/{mug}"), None).await;
    assert_eq!(
        (&admin["title"], &admin["variants"][0]["title"]),
        (&json!("Mug"), &json!("S / Red"))
    );

    // An outdated translation is still served.
    let path = format!("/admin/products/{mug}");
    let (status, json) = send(&app, "PATCH", &path, Some(r#"{"title":"Coffee mug"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (_, product) = read_ok(&app, &pk, &format!("{by_handle}&language=de")).await;
    assert_eq!(product["title"], "Tasse");

    // Unknown parameters are refused as on every route.
    let (status, _, body) = read(&app, &pk, &format!("{by_handle}&lang=de")).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_query")
    );
}

#[tokio::test]
async fn a_read_falls_back_to_its_markets_default_language() {
    let app = app().await;
    languages(&app, &[("de", true), ("de-AT", true)]).await;
    let (mug, _) = sized_mug(&app).await;
    translate(&app, "product", &mug, "de", &[("title", "Tasse")]).await;
    translate(&app, "product", &mug, "de-AT", &[("title", "Häferl")]).await;
    market_offers(&app, &["de", "en"]).await;
    let pk = publishable(&app).await;
    let title = async |query: &str| {
        let uri = format!("/storefront/products/by-handle/mug{query}");
        let (language, product) = read_ok(&app, &pk, &uri).await;
        (language, product["title"].as_str().unwrap().to_string())
    };
    let (german, english, austrian) = (
        ("de".to_string(), "Tasse".to_string()),
        ("en".to_string(), "Mug".to_string()),
        ("de-AT".to_string(), "Häferl".to_string()),
    );

    // In Croatia's market German comes first.
    assert_eq!(title("?country=HR").await, german);
    assert_eq!(title("?country=HR&language=en").await, english);
    // Published by the store, not offered by the market: its default, not the
    // language the tag belongs to by chance.
    assert_eq!(title("?country=HR&language=de-AT").await, german);
    assert_eq!(title("?country=HR&language=fr").await, german);

    // A country no market sells to, or none: what the store has published, its own
    // language otherwise.
    for place in ["?country=DE", "?"] {
        assert_eq!(title(place).await, english, "{place}");
        assert_eq!(
            title(&format!("{place}&language=de-AT")).await,
            austrian,
            "{place}"
        );
        assert_eq!(
            title(&format!("{place}&language=fr")).await,
            english,
            "{place}"
        );
    }

    // The reads that price nothing take the country for its market's languages alone,
    // so a page's reads agree.
    let (status, json) = send(
        &app,
        "POST",
        "/admin/categories",
        Some(r#"{"name":"Mugs","handle":"mugs","visible":true}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    for route in [
        "/storefront/categories",
        "/storefront/categories/by-handle/mugs",
        "/storefront/custom-field-definitions",
        "/storefront/store",
    ] {
        let language = async |query: &str| read_ok(&app, &pk, &format!("{route}{query}")).await.0;
        assert_eq!(language("").await, "en", "{route}");
        assert_eq!(language("?country=HR").await, "de", "{route}");
        assert_eq!(language("?country=HR&language=en").await, "en", "{route}");
        assert_eq!(
            language("?country=HR&language=de-AT").await,
            "de",
            "{route}"
        );
        assert_eq!(language("?country=DE").await, "en", "{route}");
        assert_eq!(
            language("?country=DE&language=de-AT").await,
            "de-AT",
            "{route}"
        );
        // A country code that isn't one is refused as the product read refuses it.
        let (status, _, body) = read(&app, &pk, &format!("{route}?country=Croatia")).await;
        let product = "/storefront/products/by-handle/mug?country=Croatia";
        let (expected, _, product) = read(&app, &pk, product).await;
        assert_eq!((status, code(&body)), (expected, code(&product)), "{route}");
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{route}");
    }
}

#[tokio::test]
async fn an_unpublished_language_is_never_served_not_as_a_fallback_either() {
    let app = app().await;
    languages(&app, &[("de", false), ("de-AT", true)]).await;
    let (mug, _) = sized_mug(&app).await;
    translate(&app, "product", &mug, "de", &[("title", "Tasse")]).await;
    translate(
        &app,
        "product",
        &mug,
        "de-AT",
        &[("description", "<p>Für Kaffee.</p>")],
    )
    .await;
    let pk = publishable(&app).await;
    let uri = |language: &str| format!("/storefront/products/by-handle/mug?language={language}");

    let (language, product) = read_ok(&app, &pk, &uri("de-AT")).await;
    assert_eq!(language, "de-AT");
    assert_eq!(product["description"], "<p>Für Kaffee.</p>");
    assert_eq!(product["title"], "Mug", "`de` isn't published: skipped");
    let (language, product) = read_ok(&app, &pk, &uri("de")).await;
    assert_eq!(
        (language.as_str(), &product["title"]),
        ("en", &json!("Mug"))
    );

    // Once published, it's the fallback.
    languages(&app, &[("de", true), ("de-AT", true)]).await;
    let (_, product) = read_ok(&app, &pk, &uri("de-AT")).await;
    assert_eq!(product["title"], "Tasse");
}

#[tokio::test]
async fn a_products_media_and_promotions_are_translated() {
    let app = app_with_media().await;
    languages(&app, &[("de", true)]).await;
    market_selling_a_mug(&app).await;
    market_offers(&app, &["en", "de"]).await;
    let (_, mugs) = send(&app, "GET", "/admin/products?limit=1", None).await;
    let mug = mugs["items"][0]["id"].as_str().unwrap().to_string();
    let (described, plain) = (ready_png(&app).await, ready_png(&app).await);
    let path = format!("/admin/assets/{described}");
    let (status, json) = send(&app, "PATCH", &path, Some(r#"{"alt":"A blue mug"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let body = json!({ "assets": [described, plain] }).to_string();
    let path = format!("/admin/products/{mug}/media");
    let (status, json) = send(&app, "PUT", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let discount = json!({"title": "3 for 2", "method": "automatic", "kind": {"type": "buy_x_get_y",
        "buys": {"items": {"type": "all"}}, "buy": {"type": "quantity", "quantity": 2},
        "gets": {"items": {"type": "all"}}, "get_quantity": 1,
        "value": {"type": "percent", "basis_points": 10000}}});
    let (status, discount) = send(
        &app,
        "POST",
        "/admin/discounts",
        Some(&discount.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{discount}");
    let discount = discount["id"].as_str().unwrap();
    translate(
        &app,
        "asset",
        &described,
        "de",
        &[("alt", "Eine blaue Tasse")],
    )
    .await;
    translate(&app, "discount", discount, "de", &[("title", "3 für 2")]).await;
    let pk = publishable(&app).await;
    let uri = "/storefront/products/by-handle/mug?country=HR";

    let (language, product) = read_ok(&app, &pk, &format!("{uri}&language=de")).await;
    assert_eq!(language, "de");
    assert_eq!(
        (&product["media"][0]["alt"], &product["media"][1]["alt"]),
        (&json!("Eine blaue Tasse"), &Value::Null)
    );
    assert_eq!(product["promotions"][0]["title"], "3 für 2");
    assert_eq!(product["title"], "Mug", "nothing translated: the source");

    let (language, product) = read_ok(&app, &pk, uri).await;
    assert_eq!(language, "en");
    assert_eq!(product["media"][0]["alt"], "A blue mug");
    assert_eq!(product["promotions"][0]["title"], "3 for 2");
}

#[tokio::test]
async fn categories_are_read_in_the_language_asked_for() {
    let app = app_with_media().await;
    languages(&app, &[("de", true), ("fr", false)]).await;
    let category = async |body: Value| {
        let (status, json) = send(&app, "POST", "/admin/categories", Some(&body.to_string())).await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        json["id"].as_str().unwrap().to_string()
    };
    let home = category(json!({"name": "Home", "handle": "home", "visible": true})).await;
    let lighting = category(json!({
        "name": "Lighting", "handle": "lighting", "visible": true, "parent": home,
        "description": "<p>Lamps and bulbs.</p>",
    }))
    .await;
    let lamps = category(json!({
        "name": "Lamps", "handle": "lamps", "visible": true, "parent": lighting,
    }))
    .await;
    let bulbs = category(json!({
        "name": "Bulbs", "handle": "bulbs", "visible": true, "parent": lighting,
    }))
    .await;
    let image = ready_png(&app).await;
    let path = format!("/admin/assets/{image}");
    let (status, json) = send(&app, "PATCH", &path, Some(r#"{"alt":"A lamp"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let path = format!("/admin/categories/{lighting}/image");
    let body = json!({ "image": image }).to_string();
    let (status, json) = send(&app, "PUT", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");

    translate(&app, "category", &home, "de", &[("name", "Zuhause")]).await;
    translate(
        &app,
        "category",
        &lighting,
        "de",
        &[
            ("name", "Beleuchtung"),
            ("description", "<p>Lampen und Birnen.</p>"),
        ],
    )
    .await;
    translate(&app, "category", &lamps, "de", &[("name", "Lampen")]).await;
    translate(&app, "asset", &image, "de", &[("alt", "Eine Lampe")]).await;
    translate(&app, "category", &home, "fr", &[("name", "Maison")]).await;
    let pk = publishable(&app).await;

    // The tree.
    let (language, tree) = read_ok(&app, &pk, "/storefront/categories?language=de").await;
    assert_eq!(language, "de");
    let top = &tree["items"][0];
    assert_eq!(
        (&top["name"], &top["handle"]),
        (&json!("Zuhause"), &json!("home"))
    );
    let below = &top["children"][0];
    assert_eq!(below["name"], "Beleuchtung");
    assert_eq!(
        (&below["children"][0]["name"], &below["children"][1]["name"]),
        (&json!("Lampen"), &json!("Bulbs")),
        "a category without a translation keeps its name"
    );
    // No country here: the store's default unless a published language is asked for.
    for query in ["", "?language=en", "?language=fr", "?language=xx-"] {
        let uri = format!("/storefront/categories{query}");
        let (language, tree) = read_ok(&app, &pk, &uri).await;
        assert_eq!(language, "en", "{query:?}");
        assert_eq!(tree["items"][0]["name"], "Home", "{query:?}");
    }

    // The page.
    let page = "/storefront/categories/by-handle/lighting";
    let (language, lighting) = read_ok(&app, &pk, &format!("{page}?language=de")).await;
    assert_eq!(language, "de");
    assert_eq!(lighting["name"], "Beleuchtung");
    assert_eq!(lighting["description"], "<p>Lampen und Birnen.</p>");
    assert_eq!(lighting["image"]["alt"], "Eine Lampe");
    assert_eq!(lighting["ancestors"][0]["name"], "Zuhause");
    assert_eq!(
        (
            &lighting["children"][0]["name"],
            &lighting["children"][1]["name"]
        ),
        (&json!("Lampen"), &json!("Bulbs"))
    );
    let _ = bulbs;
    let (language, lighting) = read_ok(&app, &pk, page).await;
    assert_eq!(language, "en");
    assert_eq!(lighting["name"], "Lighting");
    assert_eq!(lighting["image"]["alt"], "A lamp");
    assert_eq!(lighting["ancestors"][0]["name"], "Home");

    // The admin's tree is the source.
    let (_, admin) = send(&app, "GET", "/admin/categories", None).await;
    assert_eq!(admin["items"][0]["name"], "Home");
}

#[tokio::test]
async fn custom_field_definitions_are_read_in_the_language_asked_for() {
    let app = app().await;
    languages(&app, &[("de", true), ("fr", false)]).await;
    let definition = async |body: Value| {
        let uri = "/admin/custom-field-definitions";
        let (status, json) = send(&app, "POST", uri, Some(&body.to_string())).await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        json["id"].as_str().unwrap().to_string()
    };
    let shown = json!({"admin": "read_write", "storefront": "read"});
    let material = definition(json!({
        "owner_type": "product", "key": "material", "name": "Material", "type": "choice",
        "description": "What it's made of.",
        "validations": {"choices": [
            {"key": "wool", "label": "Wool"}, {"key": "clay", "label": "Clay"},
        ]},
        "access": shown,
    }))
    .await;
    definition(json!({
        "owner_type": "category", "key": "season", "name": "Season", "type": "text",
        "access": shown,
    }))
    .await;
    translate(
        &app,
        "custom_field_definition",
        &material,
        "de",
        &[("name", "Werkstoff"), ("choices.wool.label", "Wolle")],
    )
    .await;
    let pk = publishable(&app).await;
    let uri = "/storefront/custom-field-definitions";

    let (language, list) = read_ok(&app, &pk, &format!("{uri}?language=de")).await;
    assert_eq!(language, "de");
    let material = &list["items"][0];
    assert_eq!(
        (
            &material["field"],
            &material["name"],
            &material["description"]
        ),
        (
            &json!("custom.material"),
            &json!("Werkstoff"),
            &json!("What it's made of.")
        )
    );
    assert_eq!(
        material["choices"],
        json!([{"key": "wool", "label": "Wolle"}, {"key": "clay", "label": "Clay"}])
    );
    assert_eq!(list["items"][1]["name"], "Season");

    // With the route's own filter beside it.
    let (language, list) =
        read_ok(&app, &pk, &format!("{uri}?owner_type=product&language=de")).await;
    assert_eq!(
        (language.as_str(), &list["items"][0]["name"]),
        ("de", &json!("Werkstoff"))
    );
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    for query in ["", "?language=fr", "?owner_type=product"] {
        let (language, list) = read_ok(&app, &pk, &format!("{uri}{query}")).await;
        assert_eq!(language, "en", "{query:?}");
        assert_eq!(list["items"][0]["name"], "Material", "{query:?}");
        assert_eq!(list["items"][0]["choices"][0]["label"], "Wool", "{query:?}");
    }
}

#[tokio::test]
async fn the_stores_description_is_read_in_the_language_asked_for() {
    let app = app().await;
    languages(&app, &[("de", true), ("fr", false)]).await;
    let body = r#"{"description":"Mugs for every morning."}"#;
    let (status, store) = send(&app, "PATCH", "/admin/store", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{store}");
    let id = store["id"].as_str().unwrap();
    translate(
        &app,
        "store",
        id,
        "de",
        &[("description", "Tassen für jeden Morgen.")],
    )
    .await;
    translate(&app, "store", id, "fr", &[("description", "Des tasses.")]).await;
    let pk = publishable(&app).await;

    let (language, store) = read_ok(&app, &pk, "/storefront/store?language=de").await;
    assert_eq!(language, "de");
    assert_eq!(store["description"], "Tassen für jeden Morgen.");
    assert_eq!(store["default_language"], "en");
    for query in ["", "?language=en", "?language=fr"] {
        let (language, store) = read_ok(&app, &pk, &format!("/storefront/store{query}")).await;
        assert_eq!(language, "en", "{query:?}");
        assert_eq!(store["description"], "Mugs for every morning.", "{query:?}");
    }
    let (status, _, body) = read(&app, &pk, "/storefront/store?lang=de").await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_query")
    );
}
