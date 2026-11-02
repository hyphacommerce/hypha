//! Address autocomplete: the store's autocomplete app suggesting addresses while someone
//! types, resolving the one picked through the country's format, and the limits that keep
//! a scraper from spending the merchant's lookups.

use serde_json::{Value, json};

use super::*;

const SUGGESTIONS: &str = "/storefront/address-suggestions?q=Ilica&country=HR&session=s1";

fn two_suggestions() -> Value {
    json!({"suggestions": [
        {"id": "p1", "label": "Ilica 1", "description": "10000 Zagreb, Croatia"},
        {"id": "p2", "label": "Ilica 12"}
    ]})
}

/// The autocomplete app answering `suggest` and `resolve`, named in the checkout settings.
async fn suggesting(app: &TestApp, suggest: Value, resolve: Value) -> (AppAnswers, AppSaw) {
    let (base, answers, saw) =
        fake_app(&[("/suggest", 200, suggest), ("/resolve", 200, resolve)]).await;
    app_answering(
        app,
        "app_places",
        &base,
        &[
            ("address.suggest", "/suggest"),
            ("address.resolve", "/resolve"),
        ],
    )
    .await;
    let patch = r#"{"autocomplete_app":"app_places"}"#;
    let (status, json) = send(app, "PATCH", "/admin/checkout-settings", Some(patch)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["autocomplete_app"], "app_places");
    (answers, saw)
}

#[tokio::test]
async fn without_an_app_there_are_no_suggestions() {
    let app = app().await;
    let pk = publishable(&app).await;
    let (status, json) = send_as(&app, &pk, "GET", SUGGESTIONS, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "autocomplete_unavailable")
    );
    let (status, form) = send_as(&app, &pk, "GET", "/storefront/address-formats/HR", None).await;
    assert_eq!(status, StatusCode::OK, "{form}");
    assert_eq!(form["autocomplete"], false);
}

#[tokio::test]
async fn an_app_answering_only_suggestions_isnt_an_autocomplete_app() {
    let app = app().await;
    let (base, _, _) = fake_app(&[("/suggest", 200, two_suggestions())]).await;
    app_answering(
        &app,
        "app_places",
        &base,
        &[("address.suggest", "/suggest")],
    )
    .await;
    let patch = r#"{"autocomplete_app":"app_places"}"#;
    let (status, json) = send(&app, "PATCH", "/admin/checkout-settings", Some(patch)).await;
    assert_eq!(
        (status, code(&json)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "autocomplete_app_unavailable"
        )
    );
}

#[tokio::test]
async fn suggestions_come_from_the_app_with_what_was_typed() {
    let app = app().await;
    let pk = publishable(&app).await;
    let (_, saw) = suggesting(&app, two_suggestions(), json!({})).await;
    let (status, json) = send_as(&app, &pk, "GET", SUGGESTIONS, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json,
        json!({"suggestions": [
            {"id": "p1", "label": "Ilica 1", "description": "10000 Zagreb, Croatia"},
            {"id": "p2", "label": "Ilica 12", "description": null}
        ]})
    );
    let saw = saw.lock().unwrap();
    assert_eq!(saw.len(), 1);
    assert_eq!(saw[0].1, "address.suggest");
    assert_eq!(
        saw[0].3,
        json!({"query": "Ilica", "country": "HR", "session": "s1", "limit": 5})
    );
}

#[tokio::test]
async fn the_form_says_the_store_suggests() {
    let app = app().await;
    let pk = publishable(&app).await;
    suggesting(&app, two_suggestions(), json!({})).await;
    let (status, form) = send_as(&app, &pk, "GET", "/storefront/address-formats/HR", None).await;
    assert_eq!(status, StatusCode::OK, "{form}");
    assert_eq!(form["autocomplete"], true);
}

#[tokio::test]
async fn a_failing_app_suggests_nothing() {
    let app = app().await;
    let pk = publishable(&app).await;
    let (answers, _) = suggesting(&app, two_suggestions(), json!({})).await;
    answers
        .lock()
        .unwrap()
        .insert("/suggest".to_string(), (500, json!({})));
    let (status, json) = send_as(&app, &pk, "GET", SUGGESTIONS, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json, json!({"suggestions": []}));
}

#[tokio::test]
async fn more_suggestions_than_asked_for_are_none() {
    let app = app().await;
    let pk = publishable(&app).await;
    suggesting(&app, two_suggestions(), json!({})).await;
    let path = format!("{SUGGESTIONS}&limit=1");
    let (status, json) = send_as(&app, &pk, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json, json!({"suggestions": []}));
}

#[tokio::test]
async fn two_letters_are_too_few_to_look_up() {
    let app = app().await;
    let pk = publishable(&app).await;
    suggesting(&app, two_suggestions(), json!({})).await;
    let path = "/storefront/address-suggestions?q=Il&country=HR&session=s1";
    let (status, json) = send_as(&app, &pk, "GET", path, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_suggestion_query")
    );
    assert_eq!(json["details"], json!({"parameter": "q"}));
}

#[tokio::test]
async fn a_picked_suggestion_is_resolved_and_normalised() {
    let app = app().await;
    let pk = publishable(&app).await;
    let resolved = json!({"address": {"line1": "1600 Amphitheatre Pkwy",
        "city": "Mountain View", "province": "California", "postal_code": "94043",
        "country": "US"}});
    let (_, saw) = suggesting(&app, two_suggestions(), resolved).await;
    let path = "/storefront/address-suggestions/p1?country=US&session=s1";
    let (status, json) = send_as(&app, &pk, "GET", path, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["address"]["province"], "CA");
    assert_eq!(json["address"]["province_name"], "California");
    assert_eq!(json["problems"], json!([]));
    let saw = saw.lock().unwrap();
    assert_eq!(saw[0].1, "address.resolve");
    assert_eq!(
        saw[0].3,
        json!({"id": "p1", "country": "US", "session": "s1"})
    );
}

#[tokio::test]
async fn a_resolved_address_missing_its_postal_code_says_so() {
    let app = app().await;
    let pk = publishable(&app).await;
    let resolved = json!({"address": {"line1": "Ilica 1", "city": "Zagreb", "country": "HR"}});
    suggesting(&app, two_suggestions(), resolved).await;
    let path = "/storefront/address-suggestions/p1?country=HR&session=s1";
    let (status, json) = send_as(&app, &pk, "GET", path, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["address"]["line1"], "Ilica 1");
    assert_eq!(json["problems"][0]["field"], "postal_code");
    assert_eq!(json["problems"][0]["code"], "required");
}

#[tokio::test]
async fn an_address_resolved_in_another_country_is_the_apps_failure() {
    let app = app().await;
    let pk = publishable(&app).await;
    let resolved = json!({"address": {"line1": "Slovenska 1", "city": "Ljubljana",
        "postal_code": "1000", "country": "SI"}});
    suggesting(&app, two_suggestions(), resolved).await;
    let path = "/storefront/address-suggestions/p1?country=HR&session=s1";
    let (status, json) = send_as(&app, &pk, "GET", path, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::BAD_GATEWAY, "app_unavailable")
    );
    assert_eq!(
        json["details"],
        json!({"app": "app_places", "point": "address.resolve"})
    );
}

#[tokio::test]
async fn one_typing_session_is_rate_limited() {
    let app = app().await;
    let pk = publishable(&app).await;
    suggesting(&app, json!({"suggestions": []}), json!({})).await;
    let (burst, _) = hypha_http::rate_limit::Rule::AddressSuggestSession.quota(&Default::default());
    for _ in 0..burst {
        let (status, json) = send_as(&app, &pk, "GET", SUGGESTIONS, None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
    }
    let (status, json) = send_as(&app, &pk, "GET", SUGGESTIONS, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    );
    // Another session from the same place goes on.
    let other = "/storefront/address-suggestions?q=Ilica&country=HR&session=s2";
    let (status, json) = send_as(&app, &pk, "GET", other, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

#[tokio::test]
async fn staff_get_suggestions_too() {
    let app = app().await;
    suggesting(&app, two_suggestions(), json!({})).await;
    let path = "/admin/address-suggestions?q=Ilica&country=HR&session=s1";
    let (status, json) = send(&app, "GET", path, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["suggestions"].as_array().unwrap().len(), 2);
}
