//! Addresses against their country's format: the forms, problems by field, regions as
//! codes.

use serde_json::json;

use super::*;

#[tokio::test]
async fn a_countrys_form_follows_its_format() {
    let app = app().await;
    let pk = publishable(&app).await;
    let (status, form) = send_as(&app, &pk, "GET", "/storefront/address-formats/US", None).await;
    assert_eq!(status, StatusCode::OK, "{form}");
    assert_eq!(form["rows"][4], json!(["city", "province", "postal_code"]));
    assert_eq!(
        form["required"],
        json!(["last_name", "line1", "city", "province", "postal_code"])
    );
    assert_eq!(form["labels"]["province"], "State");
    assert_eq!(form["labels"]["postal_code"], "ZIP code");
    assert_eq!(form["postal_code"], "required");
    assert_eq!(form["postal_code_example"], "90210");
    assert_eq!(form["regions"][0], json!({"code": "AL", "name": "Alabama"}));
    assert_eq!(form["autocomplete"], false);
}

#[tokio::test]
async fn a_country_without_regions_lists_none() {
    let app = app().await;
    let (status, form) = send(&app, "GET", "/admin/address-formats/HR", None).await;
    assert_eq!(status, StatusCode::OK, "{form}");
    assert_eq!(form["regions"], json!([]));
    assert_eq!(
        form["required"],
        json!(["last_name", "line1", "postal_code", "city"])
    );
}

#[tokio::test]
async fn an_unknown_country_has_no_form() {
    let app = app().await;
    for country in ["ZZ", "us", "USA"] {
        let path = format!("/admin/address-formats/{country}");
        let (status, json) = send(&app, "GET", &path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{country}: {json}");
    }
}

async fn customer(app: &TestApp) -> String {
    let body = json!({"email": "ana@example.com"}).to_string();
    let (status, customer) = send(app, "POST", "/admin/customers", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{customer}");
    customer["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn every_problem_comes_back_by_field() {
    let app = app().await;
    let path = format!("/admin/customers/{}/addresses", customer(&app).await);
    let body = json!({"last_name": "Horvat", "line1": "1 Main St", "city": " ",
        "province": "Kalifornia", "postal_code": "1", "country": "US"})
    .to_string();
    let (status, json) = send(&app, "POST", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    assert_eq!(json["code"], "invalid_address");
    let problems: Vec<(&str, &str)> = json["details"]["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["field"].as_str().unwrap(), p["code"].as_str().unwrap()))
        .collect();
    assert_eq!(
        problems,
        [
            ("city", "required"),
            ("province", "unknown_region"),
            ("postal_code", "invalid_format"),
        ]
    );
    assert_eq!(
        json["details"]["problems"][2]["message"],
        "Enter a valid ZIP code, like 90210"
    );
}

#[tokio::test]
async fn a_region_is_stored_as_its_code_and_named_beside_it() {
    let app = app().await;
    let path = format!("/admin/customers/{}/addresses", customer(&app).await);
    let body = json!({"last_name": "Horvat", "line1": "1600 Amphitheatre Pkwy",
        "city": "Mountain View", "province": "california", "postal_code": "94043",
        "country": "US"})
    .to_string();
    let (status, json) = send(&app, "POST", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    let address = &json["addresses"][0];
    assert_eq!(address["province"], "CA");
    assert_eq!(address["province_name"], "California");
}

#[tokio::test]
async fn a_cart_address_is_checked_and_normalised() {
    let app = app().await;
    let (pk, cart, _) = cart_for_three_mugs(&app).await;
    let path = format!("{cart}/shipping-address");
    let address = |postal_code: &str| {
        json!({"address": {"last_name": "Horvat", "line1": "Ilica 1", "city": "Zagreb",
            "postal_code": postal_code, "country": "HR"}})
        .to_string()
    };
    let (status, json) = send_as(&app, &pk, "PUT", &path, Some(&address("1"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    assert_eq!(json["details"]["problems"][0]["field"], "postal_code");
    let (status, json) = send_as(&app, &pk, "PUT", &path, Some(&address("hr-10 000"))).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["shipping_address"]["postal_code"], "HR-10 000");
    assert_eq!(json["shipping_address"]["province_name"], Value::Null);
}

#[tokio::test]
async fn a_location_address_is_checked() {
    let app = app().await;
    let (_, locations) = send(&app, "GET", "/admin/locations", None).await;
    let path = format!(
        "/admin/locations/{}",
        locations["items"][0]["id"].as_str().unwrap()
    );
    let body = json!({"address": {"last_name": "Warehouse", "line1": "Ilica 1",
        "city": "Zagreb", "country": "HR"}})
    .to_string();
    let (status, json) = send(&app, "PATCH", &path, Some(&body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    assert_eq!(
        json["details"]["problems"],
        json!([{"field": "postal_code", "code": "required", "message": "Enter a postal code"}])
    );
}
