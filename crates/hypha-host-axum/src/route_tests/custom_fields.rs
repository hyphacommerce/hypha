//! Custom fields: definitions, an owner's values, the bulk routes, list filters, redaction
//! of personal fields, storefront visibility and the webhook entries an app may read.

use serde_json::{Value, json};

use super::*;

/// Creates a `custom` definition as the admin; returns it.
async fn define(app: &TestApp, body: Value) -> Value {
    let (status, created) = send(
        app,
        "POST",
        "/admin/custom-field-definitions",
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created
}

async fn patch_fields(app: &TestApp, path: &str, body: Value) -> (StatusCode, Value) {
    send(app, "PATCH", path, Some(&body.to_string())).await
}

/// The IDs a product list finds for `query`, sorted.
async fn found(app: &TestApp, query: &str) -> Vec<String> {
    let (status, page) = send(app, "GET", &format!("/admin/products?{query}"), None).await;
    assert_eq!(status, StatusCode::OK, "{query}: {page}");
    let mut ids: Vec<String> = super::ids(&page).iter().map(|s| s.to_string()).collect();
    ids.sort();
    ids
}

async fn customer(app: &TestApp, email: &str) -> String {
    let body = json!({ "email": email }).to_string();
    let (status, created) = send(app, "POST", "/admin/customers", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn definitions_are_created_listed_changed_moved_and_deleted() {
    let app = app().await;
    let material = define(
        &app,
        json!({"owner_type": "product", "key": "material", "type": "text", "name": "Material",
               "capabilities": {"filterable": true}}),
    )
    .await;
    assert_eq!(material["field"], "custom.material");
    assert_eq!(material["namespace"], "custom");
    assert_eq!(material["type"], "text");
    assert_eq!(material["position"], 0);
    assert_eq!(material["managed_by"], Value::Null);
    assert_eq!(
        material["access"],
        json!({"admin": "read_write", "storefront": "none"})
    );
    let care = define(
        &app,
        json!({"owner_type": "product", "key": "care", "type": "multiline_text", "name": "Care"}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "customer", "key": "tier", "type": "choice", "name": "Tier",
               "validations": {"choices": [{"key": "gold", "label": "Gold"}]}}),
    )
    .await;

    // The same key on the same owner type is taken; on another one it isn't.
    let (status, json) = send(
        &app,
        "POST",
        "/admin/custom-field-definitions",
        Some(
            &json!({"owner_type": "product", "key": "material", "type": "text", "name": "M"})
                .to_string(),
        ),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "custom_field_exists")
    );
    let (status, json) = send(
        &app,
        "POST",
        "/admin/custom-field-definitions",
        Some(
            &json!({"owner_type": "product", "key": "x", "type": "colour", "name": "X"})
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");

    let (_, products) = send(
        &app,
        "GET",
        "/admin/custom-field-definitions?owner_type=product",
        None,
    )
    .await;
    let keys: Vec<&str> = products["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["material", "care"]);
    let (_, all) = send(&app, "GET", "/admin/custom-field-definitions", None).await;
    assert_eq!(all["items"].as_array().unwrap().len(), 3);

    let id = material["id"].as_str().unwrap();
    let path = format!("/admin/custom-field-definitions/{id}");
    let (status, changed) = send(
        &app,
        "PATCH",
        &path,
        Some(r#"{"name": "Fabric", "description": "What it's made of"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["name"], "Fabric");
    assert_eq!(changed["description"], "What it's made of");

    let (status, moved) = send(
        &app,
        "PUT",
        &format!("{path}/position"),
        Some(r#"{"position": 9}"#),
    )
    .await;
    assert_eq!((status, &moved["position"]), (StatusCode::OK, &json!(1)));
    let (_, care) = send(
        &app,
        "GET",
        &format!(
            "/admin/custom-field-definitions/{}",
            care["id"].as_str().unwrap()
        ),
        None,
    )
    .await;
    assert_eq!(care["position"], 0);

    // Scopes: reading needs custom_fields:read, writing custom_fields:write.
    let catalog = key(
        &app,
        json!({"kind": "secret", "name": "catalog", "scopes": ["products:write"]}),
    )
    .await;
    let (status, json) = send_as(&app, &catalog, "GET", &path, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );
    let reader = key(
        &app,
        json!({"kind": "secret", "name": "r", "scopes": ["custom_fields:read"]}),
    )
    .await;
    let (status, _) = send_as(&app, &reader, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, json) = send_as(&app, &reader, "DELETE", &path, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );

    let (status, _) = send(&app, "DELETE", &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, json) = send(&app, "GET", &path, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "custom_field_definition_not_found")
    );
}

#[tokio::test]
async fn an_apps_fields_are_read_only_through_the_definitions_api() {
    let app = app().await;
    super::create_app(&app, json!({"key": "app_erp", "name": "ERP"})).await;
    let ports = app.state.public_ports().await.unwrap();
    let spec = hypha_custom_fields::DefinitionSpec {
        owner_type: hypha_custom_fields::OwnerType::Product,
        key: "erp_id".to_string(),
        field_type: hypha_custom_fields::FieldType::parse("text").unwrap(),
        name: "ERP ID".to_string(),
        description: None,
        validations: Default::default(),
        access: hypha_custom_fields::Access {
            admin: hypha_custom_fields::AdminAccess::Read,
            storefront: hypha_custom_fields::StorefrontAccess::None,
        },
        private: true,
        personal: false,
        capabilities: Default::default(),
    };
    hypha_custom_fields::sync_managed_definitions(
        &Ctx::system(),
        &ports,
        &UuidV7Ids,
        &hypha_custom_fields::Namespace::parse("app_erp").unwrap(),
        vec![spec],
    )
    .await
    .unwrap();
    drop(ports);
    let (_, all) = send(&app, "GET", "/admin/custom-field-definitions", None).await;
    let erp = &all["items"][0];
    assert_eq!(erp["field"], "app_erp.erp_id");
    assert_eq!(erp["managed_by"], "app_erp");
    let path = format!(
        "/admin/custom-field-definitions/{}",
        erp["id"].as_str().unwrap()
    );
    let (status, json) = send(&app, "PATCH", &path, Some(r#"{"name": "Mine"}"#)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "custom_field_managed")
    );
    let (status, json) = send(&app, "DELETE", &path, None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "custom_field_managed")
    );

    // Staff may read it but not write it (`access.admin: read`).
    let product = create_product(&app, "mug").await;
    let (status, json) = patch_fields(
        &app,
        &format!("/admin/products/{product}/custom-fields"),
        json!({"custom_fields": {"app_erp.erp_id": "A-1"}}),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "custom_fields_invalid")
    );
    assert_eq!(json["details"][0]["code"], "custom_field_read_only");
}

#[tokio::test]
async fn a_products_and_its_variants_values_are_read_and_written() {
    let app = app().await;
    define(
        &app,
        json!({"owner_type": "product", "key": "material", "type": "text", "name": "Material"}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "product", "key": "sizes", "type": "list.integer", "name": "Sizes",
               "validations": {"max": 50}}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "variant", "key": "barcode", "type": "text", "name": "Barcode"}),
    )
    .await;
    let product = create_product(&app, "mug").await;
    let path = format!("/admin/products/{product}/custom-fields");

    let (status, json) = send(&app, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["custom_fields"], json!({}));
    assert_eq!(json["owner"], product.as_str());

    let (status, json) = patch_fields(
        &app,
        &path,
        json!({"custom_fields": {"custom.material": "wool", "custom.sizes": [38, 40]}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["custom_fields"],
        json!({"custom.material": "wool", "custom.sizes": [38, 40]})
    );
    assert_eq!(json["versions"]["custom.material"], 1);

    // Every problem at once, nothing written.
    let (status, json) = patch_fields(
        &app,
        &path,
        json!({"custom_fields": {"custom.material": null, "custom.sizes": [38, 99],
                                 "custom.nope": 1}}),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "custom_fields_invalid")
    );
    let codes: Vec<(&str, &str)> = json["details"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["path"].as_str().unwrap(), p["code"].as_str().unwrap()))
        .collect();
    assert!(
        codes.contains(&("custom.nope", "custom_field_unknown")),
        "{json}"
    );
    assert!(
        codes.iter().any(|(path, _)| *path == "custom.sizes[1]"),
        "{json}"
    );

    // Optimistic writes.
    let (status, json) = patch_fields(
        &app,
        &path,
        json!({"custom_fields": {"custom.material": "linen"}, "if_versions": {"custom.material": 7}}),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "custom_field_stale")
    );
    assert_eq!(json["details"]["key"], "custom.material");
    let (status, json) = patch_fields(
        &app,
        &path,
        json!({"custom_fields": {"custom.material": null}, "if_versions": {"custom.material": 1}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["custom_fields"], json!({"custom.sizes": [38, 40]}));

    // The product's response carries them, its variants theirs.
    let (_, read) = send(&app, "GET", &format!("/admin/products/{product}"), None).await;
    let variant = read["variants"][0]["id"].as_str().unwrap().to_string();
    let (status, json) = patch_fields(
        &app,
        &format!("/admin/variants/{variant}/custom-fields"),
        json!({"custom_fields": {"custom.barcode": "4006381333931"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (_, read) = send(&app, "GET", &format!("/admin/products/{product}"), None).await;
    assert_eq!(read["custom_fields"], json!({"custom.sizes": [38, 40]}));
    assert_eq!(
        read["variants"][0]["custom_fields"],
        json!({"custom.barcode": "4006381333931"})
    );
    // A product field isn't a variant's.
    let (status, json) = patch_fields(
        &app,
        &format!("/admin/variants/{variant}/custom-fields"),
        json!({"custom_fields": {"custom.material": "wool"}}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");

    // The owner in the URL: its own 404.
    let unknown = "prod_00000000000000000000000000000099";
    let (status, json) = send(
        &app,
        "GET",
        &format!("/admin/products/{unknown}/custom-fields"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "product_not_found")
    );
    let (status, json) = send(
        &app,
        "GET",
        "/admin/variants/var_00000000000000000000000000000099/custom-fields",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "variant_not_found")
    );
    let (status, json) = send(
        &app,
        "GET",
        "/admin/categories/pcat_00000000000000000000000000000099/custom-fields",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "category_not_found")
    );

    // Writing needs the owner's write scope.
    let reader = key(
        &app,
        json!({"kind": "secret", "name": "r", "scopes": ["products:read"]}),
    )
    .await;
    let (status, json) = send_as(
        &app,
        &reader,
        "PATCH",
        &path,
        Some(r#"{"custom_fields": {"custom.material": "wool"}}"#),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );

    // One event per write, not `product.updated`.
    let events = outbox_events(&app).await;
    let types: Vec<&str> = events
        .iter()
        .map(|e| e.event_type.as_str())
        .filter(|t| t.ends_with("custom_fields_updated"))
        .collect();
    assert_eq!(types.len(), 3, "{types:?}");
}

#[tokio::test]
async fn lists_filter_by_field_values_and_carry_the_ones_asked_for() {
    let app = app().await;
    define(
        &app,
        json!({"owner_type": "product", "key": "material", "type": "text", "name": "Material",
               "capabilities": {"filterable": true}}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "product", "key": "weight", "type": "integer", "name": "Weight",
               "capabilities": {"filterable": true}}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "product", "key": "erp_id", "type": "text", "name": "ERP",
               "capabilities": {"filterable": true, "unique": true}}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "product", "key": "note", "type": "text", "name": "Note"}),
    )
    .await;
    let mug = create_product(&app, "mug").await;
    let cup = create_product(&app, "cup").await;
    let bowl = create_product(&app, "bowl").await;
    for (product, material, weight, erp) in
        [(&mug, "Wool", 300, "A-1"), (&cup, "linen", 150, "A-2")]
    {
        let (status, json) = patch_fields(
            &app,
            &format!("/admin/products/{product}/custom-fields"),
            json!({"custom_fields": {"custom.material": material, "custom.weight": weight,
                                     "custom.erp_id": erp}}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
    }
    let found = |query: &'static str| found(&app, query);
    let sorted = |mut v: Vec<&String>| {
        v.sort();
        v.into_iter().cloned().collect::<Vec<String>>()
    };
    assert_eq!(
        found("field[custom.material]=wool").await,
        sorted(vec![&mug])
    );
    assert_eq!(
        found("field[custom.material]=wool,LINEN").await,
        sorted(vec![&mug, &cup])
    );
    assert_eq!(
        found("field[custom.weight][gte]=200").await,
        sorted(vec![&mug])
    );
    assert_eq!(
        found("field[custom.weight][gt]=100&field[custom.weight][lt]=200").await,
        sorted(vec![&cup])
    );
    assert_eq!(
        found("field[custom.material][exists]=false").await,
        sorted(vec![&bowl])
    );
    // A unique field's filter is a lookup.
    assert_eq!(found("field[custom.erp_id]=a-2").await, sorted(vec![&cup]));

    // Never silently ignored.
    for (query, status, expected) in [
        (
            "field[custom.nope]=x",
            StatusCode::UNPROCESSABLE_ENTITY,
            "custom_field_unknown",
        ),
        (
            "field[custom.note]=x",
            StatusCode::UNPROCESSABLE_ENTITY,
            "custom_field_not_filterable",
        ),
        (
            "field[custom.weight][gte]=heavy",
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_custom_field_filter",
        ),
        (
            "field[custom.weight][near]=1",
            StatusCode::BAD_REQUEST,
            "invalid_query",
        ),
        (
            "field[custom.weight",
            StatusCode::BAD_REQUEST,
            "invalid_query",
        ),
        (
            "custom_fields=custom.nope",
            StatusCode::UNPROCESSABLE_ENTITY,
            "custom_field_unknown",
        ),
    ] {
        let (got, json) = send(&app, "GET", &format!("/admin/products?{query}"), None).await;
        assert_eq!((got, code(&json)), (status, expected), "{query}: {json}");
    }

    // `custom_fields` adds the values asked for; without it there are none.
    let (_, page) = send(
        &app,
        "GET",
        "/admin/products?custom_fields=custom.material,custom.weight&field[custom.material]=wool",
        None,
    )
    .await;
    assert_eq!(
        page["items"][0]["custom_fields"],
        json!({"custom.material": "Wool", "custom.weight": 300})
    );
    let (_, page) = send(&app, "GET", "/admin/products", None).await;
    assert!(page["items"][0].get("custom_fields").is_none());

    // A filtered list pages through its own filters.
    let (_, first) = send(
        &app,
        "GET",
        "/admin/products?field[custom.weight][exists]=true&limit=1",
        None,
    )
    .await;
    let cursor = first["next_cursor"].as_str().unwrap();
    let (status, json) = send(
        &app,
        "GET",
        &format!("/admin/products?limit=1&cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::BAD_REQUEST, "invalid_cursor")
    );
    let (status, next) = send(
        &app,
        "GET",
        &format!("/admin/products?field[custom.weight][exists]=true&limit=1&cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(next["items"].as_array().unwrap().len(), 1);

    // Categories: the matching ones, flat.
    define(
        &app,
        json!({"owner_type": "category", "key": "banner", "type": "text", "name": "Banner",
               "capabilities": {"filterable": true}}),
    )
    .await;
    let (_, shirts) = send(
        &app,
        "POST",
        "/admin/categories",
        Some(r#"{"name": "Shirts", "handle": "shirts", "visible": true}"#),
    )
    .await;
    send(
        &app,
        "POST",
        "/admin/categories",
        Some(r#"{"name": "Hats", "handle": "hats", "visible": true}"#),
    )
    .await;
    let shirts = shirts["id"].as_str().unwrap();
    let (status, json) = patch_fields(
        &app,
        &format!("/admin/categories/{shirts}/custom-fields"),
        json!({"custom_fields": {"custom.banner": "Summer"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (status, tree) = send(
        &app,
        "GET",
        "/admin/categories?field[custom.banner][exists]=true&custom_fields=custom.banner",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tree}");
    assert_eq!(tree["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        tree["items"][0]["custom_fields"],
        json!({"custom.banner": "Summer"})
    );
    let (_, category) = send(&app, "GET", &format!("/admin/categories/{shirts}"), None).await;
    assert_eq!(
        category["custom_fields"],
        json!({"custom.banner": "Summer"})
    );
    let (status, json) = send(&app, "GET", "/admin/categories?cursor=x", None).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::BAD_REQUEST, "invalid_query")
    );
}

#[tokio::test]
async fn bulk_routes_change_list_items_and_values_over_many_owners() {
    let app = app().await;
    let tags = define(
        &app,
        json!({"owner_type": "customer", "key": "tags", "type": "list.text", "name": "Tags",
               "validations": {"unique_items": true, "max_items": 2},
               "capabilities": {"filterable": true}}),
    )
    .await;
    let tags = tags["id"].as_str().unwrap().to_string();
    let ana = customer(&app, "ana@example.com").await;
    let bo = customer(&app, "bo@example.com").await;
    let items = format!("/admin/custom-fields/{tags}/items");

    let body = json!({"owners": [ana, bo], "add": ["vip", "VIP"]}).to_string();
    let (status, json) = send(&app, "POST", &items, Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["items"][0]["owner"], ana.as_str());
    assert_eq!(json["items"][0]["added"], json!(["vip"]));
    assert_eq!(json["items"][0]["version"], 1);
    // Again: nothing changes.
    let (_, json) = send(&app, "POST", &items, Some(&body)).await;
    assert_eq!(json["items"][1]["added"], json!([]));

    let (_, page) = send(
        &app,
        "GET",
        "/admin/customers?field[custom.tags]=VIP&custom_fields=custom.tags",
        None,
    )
    .await;
    assert_eq!(page["items"].as_array().unwrap().len(), 2, "{page}");
    assert_eq!(
        page["items"][0]["custom_fields"],
        json!({"custom.tags": ["vip"]})
    );

    // An owner whose list would outgrow `max_items` fails the whole call.
    let body = json!({"owners": [ana, bo], "add": ["a", "b"]}).to_string();
    let (status, json) = send(&app, "POST", &items, Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::CONFLICT, "too_many_items")
    );
    assert_eq!(json["details"]["max"], 2);

    // Owners are the definition's owner type, and exist.
    let body = json!({"owners": ["prod_00000000000000000000000000000001"], "add": ["x"]});
    let (status, json) = send(&app, "POST", &items, Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_owner")
    );
    let ghost = "cus_00000000000000000000000000000001";
    let body = json!({"owners": [ana, ghost], "add": ["x"]});
    let (status, json) = send(&app, "POST", &items, Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "owner_not_found")
    );
    assert_eq!(json["details"]["owners"], json!([ghost]));
    let (status, json) = send(
        &app,
        "POST",
        "/admin/custom-fields/cfd_00000000000000000000000000000001/items",
        Some(&json!({"owners": [ana], "add": ["x"]}).to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::NOT_FOUND, "custom_field_definition_not_found")
    );
    let (status, json) = send(
        &app,
        "POST",
        &items,
        Some(r#"{"owners": [], "add": ["x"]}"#),
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "owners_required")
    );
    // The owner type's write scope, before any owner is looked up.
    let reader = key(
        &app,
        json!({"kind": "secret", "name": "r", "scopes": ["customers:read"]}),
    )
    .await;
    let body = json!({"owners": [ghost], "add": ["x"]}).to_string();
    let (status, json) = send_as(&app, &reader, "POST", &items, Some(&body)).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );

    // One field on many owners.
    let tier = define(
        &app,
        json!({"owner_type": "customer", "key": "tier", "type": "choice", "name": "Tier",
               "validations": {"choices": [{"key": "gold", "label": "Gold"}]}}),
    )
    .await;
    let values = format!(
        "/admin/custom-fields/{}/values",
        tier["id"].as_str().unwrap()
    );
    let body = json!({"values": [{"owner": ana, "value": "gold"}, {"owner": bo, "value": "iron"}]});
    let (status, json) = send(&app, "POST", &values, Some(&body.to_string())).await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::UNPROCESSABLE_ENTITY, "custom_fields_invalid")
    );
    assert_eq!(json["details"][0]["owner"], bo.as_str());
    let body = json!({"values": [{"owner": ana, "value": "gold"}, {"owner": bo, "value": "gold"}]});
    let (status, json) = send(&app, "POST", &values, Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");
    let (_, read) = send(&app, "GET", &format!("/admin/customers/{bo}"), None).await;
    assert_eq!(
        read["custom_fields"],
        json!({"custom.tags": ["vip"], "custom.tier": "gold"})
    );
    let body = json!({"values": [{"owner": bo, "value": null}]});
    let (status, _) = send(&app, "POST", &values, Some(&body.to_string())).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, read) = send(
        &app,
        "GET",
        &format!("/admin/customers/{bo}/custom-fields"),
        None,
    )
    .await;
    assert_eq!(read["custom_fields"], json!({"custom.tags": ["vip"]}));

    // Values in use, for autocomplete.
    let (status, used) = send(
        &app,
        "GET",
        &format!("/admin/custom-field-definitions/{tags}/values-in-use"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{used}");
    assert_eq!(used["items"], json!([{"value": "vip", "owners": 2}]));
}

#[tokio::test]
async fn personal_fields_need_customer_data_profile() {
    let app = app().await;
    define(
        &app,
        json!({"owner_type": "customer", "key": "birthday", "type": "date", "name": "Birthday",
               "personal": true, "capabilities": {"filterable": true}}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "customer", "key": "tier", "type": "text", "name": "Tier"}),
    )
    .await;
    let ana = customer(&app, "ana@example.com").await;
    let path = format!("/admin/customers/{ana}/custom-fields");
    let (status, json) = patch_fields(
        &app,
        &path,
        json!({"custom_fields": {"custom.birthday": "1990-05-04", "custom.tier": "gold"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let crm = key(
        &app,
        json!({"kind": "secret", "name": "crm", "scopes": ["customers:write", "customer_data:email"]}),
    )
    .await;
    for read in [format!("/admin/customers/{ana}"), path.clone()] {
        let (status, json) = send_as(&app, &crm, "GET", &read, None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(
            json["custom_fields"],
            json!({"custom.tier": "gold"}),
            "{read}"
        );
        assert!(
            json["redacted"]
                .as_array()
                .unwrap()
                .contains(&json!("custom_fields.custom.birthday")),
            "{read}: {json}"
        );
        assert!(!json.to_string().contains("1990"), "{read}: {json}");
    }
    let (_, page) = send_as(
        &app,
        &crm,
        "GET",
        "/admin/customers?custom_fields=custom.birthday,custom.tier",
        None,
    )
    .await;
    let item = &page["items"][0];
    assert_eq!(item["custom_fields"], json!({"custom.tier": "gold"}));
    assert!(
        item["redacted"]
            .as_array()
            .unwrap()
            .contains(&json!("custom_fields.custom.birthday")),
        "{item}"
    );
    // Writing or filtering by it needs the scope too.
    let (status, json) = send_as(
        &app,
        &crm,
        "PATCH",
        &path,
        Some(r#"{"custom_fields": {"custom.birthday": "1991-01-01"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    assert_eq!(json["details"][0]["code"], "missing_scope");
    let (status, json) = send_as(
        &app,
        &crm,
        "GET",
        "/admin/customers?field[custom.birthday][exists]=true",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&json)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );

    // With it, and for staff, the value is there.
    let profile = key(
        &app,
        json!({"kind": "secret", "name": "p", "scopes": ["customers:read", "customer_data:profile"]}),
    )
    .await;
    let (_, json) = send_as(&app, &profile, "GET", &path, None).await;
    assert_eq!(json["custom_fields"]["custom.birthday"], "1990-05-04");
    assert!(json.get("redacted").is_none());
    let owner = owner_session(&app).await;
    let (_, json) = send_as(
        &app,
        &owner,
        "GET",
        &format!("/admin/customers/{ana}"),
        None,
    )
    .await;
    assert_eq!(json["custom_fields"]["custom.birthday"], "1990-05-04");
    assert!(json.get("redacted").is_none());
}

#[tokio::test]
async fn storefronts_see_only_the_fields_they_are_given() {
    let app = app().await;
    define(
        &app,
        json!({"owner_type": "product", "key": "material", "type": "text", "name": "Material",
               "access": {"admin": "read_write", "storefront": "read"}}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "product", "key": "cost", "type": "integer", "name": "Cost"}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "category", "key": "banner", "type": "text", "name": "Banner",
               "access": {"admin": "read_write", "storefront": "read"}}),
    )
    .await;
    let product = create_product(&app, "mug").await;
    patch_fields(
        &app,
        &format!("/admin/products/{product}/custom-fields"),
        json!({"custom_fields": {"custom.material": "clay", "custom.cost": 120}}),
    )
    .await;
    send(
        &app,
        "PATCH",
        &format!("/admin/products/{product}/status"),
        Some(r#"{"status":"active"}"#),
    )
    .await;
    publish(&app, &[&product]).await;
    let pk = publishable(&app).await;
    let (status, json) = send_as(
        &app,
        &pk,
        "GET",
        &format!("/storefront/products/{product}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["custom_fields"], json!({"custom.material": "clay"}));
    assert_eq!(json["variants"][0]["custom_fields"], json!({}));

    let (_, category) = send(
        &app,
        "POST",
        "/admin/categories",
        Some(r#"{"name": "Mugs", "handle": "mugs", "visible": true}"#),
    )
    .await;
    patch_fields(
        &app,
        &format!(
            "/admin/categories/{}/custom-fields",
            category["id"].as_str().unwrap()
        ),
        json!({"custom_fields": {"custom.banner": "Hot drinks"}}),
    )
    .await;
    let (status, json) = send_as(
        &app,
        &pk,
        "GET",
        "/storefront/categories/by-handle/mugs",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json["custom_fields"],
        json!({"custom.banner": "Hot drinks"})
    );
}

#[tokio::test]
async fn storefronts_read_the_definitions_of_the_fields_they_are_given() {
    let app = app().await;
    define(
        &app,
        json!({"owner_type": "product", "key": "cost", "type": "integer", "name": "Cost"}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "product", "key": "fit", "type": "choice", "name": "Fit",
               "validations": {"choices": [{"key": "slim", "label": "Slim fit"}]},
               "access": {"admin": "read_write", "storefront": "read"}}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "category", "key": "banner", "type": "text", "name": "Banner",
               "description": "Shown above the products.",
               "access": {"admin": "read_write", "storefront": "read"}}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "customer", "key": "tier", "type": "text", "name": "Tier",
               "access": {"admin": "read_write", "storefront": "read"}}),
    )
    .await;
    let pk = publishable(&app).await;
    let (status, json) = send_as(
        &app,
        &pk,
        "GET",
        "/storefront/custom-field-definitions",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    // The hidden cost and the customer's tier (storefronts don't read customers) are left out.
    assert_eq!(
        json["items"],
        json!([
            {"owner_type": "product", "field": "custom.fit", "name": "Fit", "description": null,
             "type": "choice", "position": 1,
             "choices": [{"key": "slim", "label": "Slim fit"}]},
            {"owner_type": "category", "field": "custom.banner", "name": "Banner",
             "description": "Shown above the products.", "type": "text", "position": 0,
             "choices": null},
        ])
    );
    let (_, json) = send_as(
        &app,
        &pk,
        "GET",
        "/storefront/custom-field-definitions?owner_type=category",
        None,
    )
    .await;
    assert_eq!(json["items"].as_array().map(Vec::len), Some(1));
    // A secret key is the wrong kind of credential for the storefront surface.
    let (status, _) = send(&app, "GET", "/storefront/custom-field-definitions", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn orders_carry_their_fields_and_filter_by_them() {
    let app = app().await;
    define(
        &app,
        json!({"owner_type": "order", "key": "erp_number", "type": "integer", "name": "ERP",
               "capabilities": {"filterable": true}}),
    )
    .await;
    let (order, _, _) = placed_order_for_three_mugs(&app).await;
    let (status, json) = patch_fields(
        &app,
        &format!("/admin/orders/{order}/custom-fields"),
        json!({"custom_fields": {"custom.erp_number": 1001}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (_, read) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    assert_eq!(read["custom_fields"], json!({"custom.erp_number": 1001}));
    let (_, page) = send(
        &app,
        "GET",
        "/admin/orders?field[custom.erp_number][gte]=1000",
        None,
    )
    .await;
    assert_eq!(super::ids(&page), [order.as_str()]);
    let (_, page) = send(
        &app,
        "GET",
        "/admin/orders?field[custom.erp_number][lt]=1000",
        None,
    )
    .await;
    assert!(super::ids(&page).is_empty());
}

#[tokio::test]
async fn webhooks_carry_only_the_entries_the_app_may_read() {
    let app = app().await;
    let (url, seen) = webhook_receiver(StatusCode::NO_CONTENT).await;
    // Made by hand: no grant, so no personal data.
    super::create_app(
        &app,
        json!({"key": "app_crm", "name": "CRM", "subscriptions": ["customer.custom_fields_updated"],
            "webhook": {"url": url}}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "customer", "key": "birthday", "type": "date", "name": "Birthday",
               "personal": true}),
    )
    .await;
    define(
        &app,
        json!({"owner_type": "customer", "key": "tier", "type": "text", "name": "Tier"}),
    )
    .await;
    let ana = customer(&app, "ana@example.com").await;
    let path = format!("/admin/customers/{ana}/custom-fields");
    // Only a personal field: nothing for the app.
    patch_fields(
        &app,
        &path,
        json!({"custom_fields": {"custom.birthday": "1990-05-04"}}),
    )
    .await;
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    assert!(seen.lock().unwrap().is_empty(), "nothing it may read");
    // Both: only the one it may read.
    patch_fields(
        &app,
        &path,
        json!({"custom_fields": {"custom.birthday": null, "custom.tier": "gold"}}),
    )
    .await;
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let body = {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        seen[0].4.clone()
    };
    let sent: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sent["type"], "customer.custom_fields_updated");
    assert_eq!(
        sent["data"]["changes"],
        json!([{"key": "custom.tier", "value": "gold"}])
    );
}

/// The well-known customer fields are offered with the body that creates them, and marked
/// taken once the merchant has a definition of their key.
#[tokio::test]
async fn suggested_fields_are_offered_until_created() {
    let app = app().await;
    let path = "/admin/custom-field-definitions/suggestions?owner_type=customer";
    let (status, list) = send(&app, "GET", path, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let items = list["items"].as_array().unwrap();
    let keys: Vec<&str> = items.iter().map(|i| i["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["tags", "note"]);
    assert!(items.iter().all(|i| i["existing"].is_null()), "{list}");
    let note = &items[1];
    assert_eq!(note["field"], "custom.note");
    assert_eq!(note["type"], "multiline_text");
    assert_eq!(note["personal"], true);
    assert_eq!(items[0]["type"], "list.text");
    assert_eq!(items[0]["capabilities"]["filterable"], true);
    assert_eq!(items[0]["validations"]["unique_items"], true);

    // Creating one is sending the suggestion back as it is, minus what describes it.
    let mut body = note.clone();
    for key in ["field", "why", "existing"] {
        body.as_object_mut().unwrap().remove(key);
    }
    let created = define(&app, body).await;
    assert_eq!(created["field"], "custom.note");
    assert_eq!(created["validations"]["max_length"], 5000);
    assert_eq!(created["personal"], true);

    let (_, list) = send(&app, "GET", path, None).await;
    assert_eq!(list["items"][1]["existing"], created["id"]);
    assert_eq!(list["items"][0]["existing"], Value::Null);

    // Taken by key alone: a merchant's own `tags` of another shape counts.
    define(
        &app,
        json!({"owner_type": "customer", "key": "tags", "type": "text", "name": "Tag"}),
    )
    .await;
    let (_, list) = send(&app, "GET", path, None).await;
    assert!(list["items"][0]["existing"].is_string(), "{list}");

    // Products have no suggestions; without an owner type, every type's come.
    let (_, none) = send(
        &app,
        "GET",
        "/admin/custom-field-definitions/suggestions?owner_type=product",
        None,
    )
    .await;
    assert_eq!(none["items"].as_array().unwrap().len(), 0);
    let (_, all) = send(
        &app,
        "GET",
        "/admin/custom-field-definitions/suggestions",
        None,
    )
    .await;
    assert_eq!(all["items"].as_array().unwrap().len(), 2);
}
