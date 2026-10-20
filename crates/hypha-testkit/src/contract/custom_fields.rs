//! What every custom fields store must do (design note 0033, §4): definitions with
//! their key, values with versions, `unique` values, list deltas applied under the
//! rows' locks, cascades, and writes staged in a batch.

use hypha_apps::{AppRepo, HasApps};
use hypha_categories::{CategoryRepo, HasCategories};
use hypha_core::{
    AppKey, CustomFieldDefinitionId, CustomerId, FieldFilter, FieldFilterOp, FilterScalar,
    HasBatch, HasOutbox, OrderId, OutboxRepo, PageLimit, ProductId, RepoError, StageEvents,
    StoredEvent, VariantId, Version, Versioned,
};
use hypha_custom_fields::{
    Access, Capabilities, CustomFieldDefinition, DefinitionRepo, DefinitionSpec, FieldChange,
    FieldType, FieldValue, HasCustomFieldDefinitions, HasCustomFieldValues, ItemsChange,
    ItemsProblem, ItemsResult, Namespace, Owner, OwnerType, RawValidations,
    StageCustomFieldDefinitions, StageCustomFieldValues, ValueCount, ValueRepo, ValueRow,
    fields_updated, keys,
};
use hypha_customers::{CustomerFilter, CustomerRepo, CustomerSort, HasCustomers};
use hypha_orders::{HasOrders, OrderFilter, OrderRepo, OrderSort};
use hypha_products::{HasProducts, ProductFilter, ProductRepo, ProductSort};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    apps::app, categories::category, customers::all_customers, orders::all_orders,
    products::all_pages, scenario,
};

fn did(n: u128) -> CustomFieldDefinitionId {
    CustomFieldDefinitionId::from_uuid(Uuid::from_u128(n))
}

fn customer(n: u128) -> Owner {
    Owner::customer(CustomerId::from_uuid(Uuid::from_u128(n)))
}

fn definition(
    n: u128,
    owner_type: OwnerType,
    namespace: &str,
    key: &str,
    field_type: &str,
) -> CustomFieldDefinition {
    let mut validations = RawValidations::default();
    if field_type.ends_with("reference") {
        validations.kind = Some("product".to_string());
    }
    CustomFieldDefinition::new(
        did(n),
        Namespace::parse(namespace).unwrap(),
        DefinitionSpec {
            owner_type,
            key: key.to_string(),
            field_type: FieldType::parse(field_type).unwrap(),
            name: format!("Field {key}"),
            description: Some("Shown on the form".to_string()),
            validations,
            access: Access::default(),
            private: false,
            personal: false,
            capabilities: Capabilities {
                filterable: FieldType::parse(field_type).unwrap().may_be_filterable(),
                unique: field_type == "text",
            },
        },
        n as u32,
    )
    .unwrap()
}

fn row(definition: &CustomFieldDefinition, owner: &Owner, json: Value) -> ValueRow {
    let value =
        FieldValue::parse(definition.field_type(), definition.validations(), &json).unwrap();
    ValueRow::new(definition, owner, &value)
}

async fn events<S: HasOutbox>(ports: &S) -> Vec<StoredEvent> {
    ports.outbox().events_after(0, 1000).await.unwrap()
}

/// Runs every definition scenario, each on fresh ports from `make`. A definition in an
/// app's or a spore's namespace needs that app stored first (adapters may refer to it:
/// removing the app removes its fields, design note 0033, §8).
pub async fn custom_field_definitions<S>(make: impl AsyncFn() -> S)
where
    S: HasApps
        + HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasOutbox
        + HasBatch<Batch: StageCustomFieldDefinitions + StageCustomFieldValues + StageEvents>,
{
    scenario("custom field definition: insert stores every field at version 1; lists sort");
    definition_round_trip(&make().await).await;
    scenario("custom field definition: (owner type, namespace, key) is unique");
    definition_keys(&make().await).await;
    scenario("custom field definition: update bumps the version; a stale copy fails");
    definition_update(&make().await).await;
    scenario("custom field definition: deleting takes its values and writes its event");
    definition_delete(&make().await).await;
    scenario("custom field definition: staged writes commit together, or not at all");
    definition_batch(&make().await).await;
    scenario("custom field definition: removing an app removes its namespace's, with values");
    app_removal(&make().await).await;
}

/// Stores the app `key`, whose namespace definitions then use.
async fn with_app<S: HasApps>(ports: &S, key: &str) {
    ports.apps().insert(&mut app(key)).await.unwrap();
}

async fn definition_round_trip<S: HasApps + HasCustomFieldDefinitions>(ports: &S) {
    with_app(ports, "app_erp").await;
    let repo = ports.custom_field_definitions();
    let mut text = definition(2, OwnerType::Customer, "custom", "tier", "text");
    let mut measured = CustomFieldDefinition::new(
        did(1),
        Namespace::of_app(AppKey::new("app_erp").unwrap()),
        DefinitionSpec {
            owner_type: OwnerType::Customer,
            key: "weight".to_string(),
            field_type: FieldType::parse("list.measurement").unwrap(),
            name: "Weight".to_string(),
            description: None,
            validations: serde_json::from_value(json!({
                "dimension": "weight",
                "max": {"value": "2", "unit": "kg"},
                "max_items": 5,
                "unique_items": true
            }))
            .unwrap(),
            access: serde_json::from_value(json!({"admin": "read", "storefront": "read"})).unwrap(),
            private: true,
            personal: true,
            capabilities: Capabilities {
                filterable: true,
                unique: false,
            },
        },
        0,
    )
    .unwrap();
    let mut product = definition(3, OwnerType::Product, "custom", "material", "text");
    repo.insert(&mut text).await.unwrap();
    repo.insert(&mut measured).await.unwrap();
    repo.insert(&mut product).await.unwrap();
    assert_eq!(measured.version(), Version::new(1));
    let stored = repo.get(did(1)).await.unwrap().unwrap();
    assert_eq!(stored, measured);
    assert!(hypha_core::Stamped::stamps(&stored).is_some());
    assert_eq!(repo.get(did(9)).await, Ok(None));
    assert_eq!(
        repo.get_by_key(OwnerType::Customer, text.namespace(), text.key())
            .await,
        Ok(Some(text.clone()))
    );
    assert_eq!(
        repo.get_by_key(OwnerType::Product, text.namespace(), text.key())
            .await,
        Ok(None)
    );
    // By owner type, then position.
    assert_eq!(
        repo.list(None).await,
        Ok(vec![product.clone(), measured.clone(), text.clone()])
    );
    assert_eq!(
        repo.list(Some(OwnerType::Customer)).await,
        Ok(vec![measured.clone(), text.clone()])
    );
    assert_eq!(
        repo.list_namespace(measured.namespace()).await,
        Ok(vec![measured])
    );
}

async fn definition_keys<S: HasApps + HasCustomFieldDefinitions>(ports: &S) {
    with_app(ports, "app_erp").await;
    let repo = ports.custom_field_definitions();
    repo.insert(&mut definition(
        1,
        OwnerType::Product,
        "custom",
        "material",
        "text",
    ))
    .await
    .unwrap();
    assert_eq!(
        repo.insert(&mut definition(
            2,
            OwnerType::Product,
            "custom",
            "material",
            "integer"
        ))
        .await,
        Err(RepoError::Duplicate(keys::CUSTOM_FIELD_KEY))
    );
    assert_eq!(
        repo.insert(&mut definition(
            1,
            OwnerType::Order,
            "custom",
            "other",
            "text"
        ))
        .await,
        Err(RepoError::Duplicate(keys::CUSTOM_FIELD_DEFINITION_ID))
    );
    // The same key on another owner type, or in another namespace, is another field.
    repo.insert(&mut definition(
        3,
        OwnerType::Variant,
        "custom",
        "material",
        "text",
    ))
    .await
    .unwrap();
    repo.insert(&mut definition(
        4,
        OwnerType::Product,
        "app_erp",
        "material",
        "text",
    ))
    .await
    .unwrap();
}

async fn definition_update<S: HasCustomFieldDefinitions>(ports: &S) {
    let repo = ports.custom_field_definitions();
    let mut d = definition(1, OwnerType::Product, "custom", "material", "text");
    repo.insert(&mut d).await.unwrap();
    let stale = d.clone();
    d.change(hypha_custom_fields::DefinitionChange {
        name: Some("Fabric".to_string()),
        validations: Some(serde_json::from_value(json!({"max_length": 40})).unwrap()),
        ..Default::default()
    })
    .unwrap();
    d.move_to(4);
    repo.update(&mut d).await.unwrap();
    assert_eq!(d.version(), Version::new(2));
    assert_eq!(repo.get(did(1)).await, Ok(Some(d)));
    let mut stale = stale;
    stale.move_to(7);
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
}

async fn definition_delete<S>(ports: &S)
where
    S: HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasOutbox
        + HasBatch<Batch: StageCustomFieldValues>,
{
    let repo = ports.custom_field_definitions();
    let mut gone = definition(1, OwnerType::Customer, "custom", "tier", "text");
    let mut kept = definition(2, OwnerType::Customer, "custom", "note", "multiline_text");
    repo.insert(&mut gone).await.unwrap();
    repo.insert(&mut kept).await.unwrap();
    let mut batch = ports.batch();
    batch.set_custom_field_value(row(&gone, &customer(5), json!("gold")), None);
    batch.set_custom_field_value(row(&kept, &customer(5), json!("hi")), None);
    ports.commit(batch).await.unwrap();
    let mut stale = gone.clone();
    stale.set_version(Version::new(7));
    assert_eq!(repo.delete(&mut stale).await, Err(RepoError::StaleVersion));
    gone.mark_deleted();
    repo.delete(&mut gone).await.unwrap();
    assert_eq!(repo.get(did(1)).await, Ok(None));
    let left = ports
        .custom_field_values()
        .get_many(&[customer(5).id()], None)
        .await
        .unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].definition, did(2));
    let types: Vec<String> = events(ports)
        .await
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert_eq!(
        types.last().map(String::as_str),
        Some("custom_field_definition.deleted")
    );
}

/// Removing an app (its second `DELETE`, design note 0033, §8) removes the definitions
/// in its namespace and their values; another app's and the merchant's stay. Storage
/// cascades record no events.
async fn app_removal<S>(ports: &S)
where
    S: HasApps
        + HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasBatch<Batch: StageCustomFieldValues>,
{
    with_app(ports, "app_erp").await;
    with_app(ports, "app_crm").await;
    let repo = ports.custom_field_definitions();
    let mut erp = definition(1, OwnerType::Customer, "app_erp", "member_id", "text");
    let mut erp_too = definition(2, OwnerType::Product, "app_erp", "sku_ref", "text");
    let mut crm = definition(3, OwnerType::Customer, "app_crm", "member_id", "text");
    let mut own = definition(4, OwnerType::Customer, "custom", "tier", "text");
    for d in [&mut erp, &mut erp_too, &mut crm, &mut own] {
        repo.insert(d).await.unwrap();
    }
    let mut batch = ports.batch();
    for d in [&erp, &crm, &own] {
        batch.set_custom_field_value(row(d, &customer(5), json!("x")), None);
    }
    ports.commit(batch).await.unwrap();

    ports
        .apps()
        .delete(&AppKey::new("app_erp").unwrap())
        .await
        .unwrap();
    assert_eq!(repo.get(did(1)).await, Ok(None));
    assert_eq!(repo.get(did(2)).await, Ok(None));
    let erp_namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    assert_eq!(repo.list_namespace(&erp_namespace).await, Ok(Vec::new()));
    assert_eq!(repo.get(did(3)).await, Ok(Some(crm)));
    assert_eq!(repo.get(did(4)).await, Ok(Some(own)));
    let mut left: Vec<CustomFieldDefinitionId> = ports
        .custom_field_values()
        .get_many(&[customer(5).id()], None)
        .await
        .unwrap()
        .into_iter()
        .map(|v| v.definition)
        .collect();
    left.sort();
    assert_eq!(left, [did(3), did(4)]);
}

async fn definition_batch<S>(ports: &S)
where
    S: HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasBatch<Batch: StageCustomFieldDefinitions + StageCustomFieldValues>,
{
    let repo = ports.custom_field_definitions();
    let mut first = definition(1, OwnerType::Product, "custom", "material", "text");
    repo.insert(&mut first).await.unwrap();

    let mut batch = ports.batch();
    let mut second = definition(2, OwnerType::Product, "custom", "care", "text");
    batch.insert_custom_field_definition(&mut second);
    first.shift_to(1);
    batch.update_custom_field_definition(&mut first);
    ports.commit(batch).await.unwrap();
    assert_eq!(second.version(), Version::new(1));
    assert_eq!(first.version(), Version::new(2));
    assert_eq!(repo.get(did(1)).await, Ok(Some(first.clone())));
    assert_eq!(repo.get(did(2)).await, Ok(Some(second.clone())));

    // A clash rolls back the whole batch.
    let mut batch = ports.batch();
    let mut third = definition(3, OwnerType::Product, "custom", "fit", "text");
    batch.insert_custom_field_definition(&mut third);
    let mut clash = definition(4, OwnerType::Product, "custom", "care", "text");
    batch.insert_custom_field_definition(&mut clash);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::CUSTOM_FIELD_KEY))
    );
    assert_eq!(repo.get(did(3)).await, Ok(None));

    // Deleting in a batch takes the values too.
    let mut batch = ports.batch();
    batch.set_custom_field_value(
        row(
            &second,
            &Owner::product(ProductId::from_uuid(Uuid::from_u128(9))),
            json!("Wash"),
        ),
        None,
    );
    ports.commit(batch).await.unwrap();
    let mut batch = ports.batch();
    second.mark_deleted();
    batch.delete_custom_field_definition(&mut second);
    ports.commit(batch).await.unwrap();
    assert_eq!(repo.get(did(2)).await, Ok(None));
    assert_eq!(
        ports
            .custom_field_values()
            .get_many(&[Uuid::from_u128(9)], None)
            .await,
        Ok(vec![])
    );
}

/// Runs every value scenario, each on fresh ports from `make`.
pub async fn custom_field_values<S>(make: impl AsyncFn() -> S)
where
    S: HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasOutbox
        + HasBatch<Batch: StageCustomFieldValues + StageEvents>,
{
    scenario("custom field value: a staged value is stored at version 1, then 2");
    value_round_trip(&make().await).await;
    scenario("custom field value: expected versions guard writes and removals");
    value_versions(&make().await).await;
    scenario("custom field value: a unique value is held by one owner per definition");
    unique_values(&make().await).await;
    scenario("custom field value: a batch commits values and events together, or not at all");
    value_batch(&make().await).await;
    scenario("custom field value: list deltas are idempotent and record what changed");
    list_deltas(&make().await).await;
    scenario("custom field value: list deltas compare text ignoring case when asked");
    list_deltas_ignore_case(&make().await).await;
    scenario("custom field value: a delta past max_items writes nothing");
    list_limits(&make().await).await;
    scenario("custom field value: pages, values in use, references, owners deleted");
    value_reads(&make().await).await;
}

async fn setup<S: HasCustomFieldDefinitions>(
    ports: &S,
    mut d: CustomFieldDefinition,
) -> CustomFieldDefinition {
    ports
        .custom_field_definitions()
        .insert(&mut d)
        .await
        .unwrap();
    d
}

async fn value_round_trip<S>(ports: &S)
where
    S: HasCustomFieldDefinitions + HasCustomFieldValues + HasBatch<Batch: StageCustomFieldValues>,
{
    let size = setup(
        ports,
        definition(1, OwnerType::Variant, "custom", "size", "decimal"),
    )
    .await;
    let product = ProductId::from_uuid(Uuid::from_u128(7));
    let owner = Owner::variant(product, VariantId::from_uuid(Uuid::from_u128(8)));
    let mut batch = ports.batch();
    batch.set_custom_field_value(row(&size, &owner, json!("12.50")), None);
    ports.commit(batch).await.unwrap();
    let values = ports.custom_field_values();
    let stored = values.get_many(&[owner.id()], None).await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].value, json!("12.50"));
    assert_eq!(stored[0].version, Version::new(1));
    assert_eq!(stored[0].owner_parent, Some(product.as_uuid()));
    assert!(stored[0].stamps.is_some());
    let mut batch = ports.batch();
    batch.set_custom_field_value(row(&size, &owner, json!("13")), None);
    ports.commit(batch).await.unwrap();
    let stored = values
        .get_many(&[owner.id()], Some(&[did(1)]))
        .await
        .unwrap();
    assert_eq!(
        (stored[0].value.clone(), stored[0].version),
        (json!("13"), Version::new(2))
    );
    assert_eq!(
        values.get_many(&[owner.id()], Some(&[did(2)])).await,
        Ok(vec![])
    );
    assert_eq!(
        values.get_many(&[Uuid::from_u128(99)], None).await,
        Ok(vec![])
    );
}

async fn value_versions<S>(ports: &S)
where
    S: HasCustomFieldDefinitions + HasCustomFieldValues + HasBatch<Batch: StageCustomFieldValues>,
{
    let tier = setup(
        ports,
        definition(1, OwnerType::Customer, "custom", "tier", "text"),
    )
    .await;
    let owner = customer(5);
    let write = async |json: Value, expected: Option<Version>| {
        let mut batch = ports.batch();
        batch.set_custom_field_value(row(&tier, &owner, json), expected);
        ports.commit(batch).await
    };
    assert_eq!(
        write(json!("gold"), Some(Version::new(1))).await,
        Err(RepoError::StaleVersion)
    );
    write(json!("gold"), Some(Version::NEW)).await.unwrap();
    assert_eq!(
        write(json!("silver"), Some(Version::NEW)).await,
        Err(RepoError::StaleVersion)
    );
    write(json!("silver"), Some(Version::new(1))).await.unwrap();
    let remove = async |expected: Option<Version>| {
        let mut batch = ports.batch();
        batch.remove_custom_field_value(tier.id(), owner.id(), expected);
        ports.commit(batch).await
    };
    assert_eq!(
        remove(Some(Version::new(1))).await,
        Err(RepoError::StaleVersion)
    );
    remove(Some(Version::new(2))).await.unwrap();
    // Removing what isn't there, unguarded, changes nothing.
    remove(None).await.unwrap();
    assert_eq!(
        ports
            .custom_field_values()
            .get_many(&[owner.id()], None)
            .await,
        Ok(vec![])
    );
}

async fn unique_values<S>(ports: &S)
where
    S: HasCustomFieldDefinitions + HasCustomFieldValues + HasBatch<Batch: StageCustomFieldValues>,
{
    let erp = setup(
        ports,
        definition(1, OwnerType::Customer, "custom", "erp_id", "text"),
    )
    .await;
    let other = setup(
        ports,
        definition(2, OwnerType::Customer, "custom", "other", "text"),
    )
    .await;
    let write = async |d: &CustomFieldDefinition, owner: Owner, json: Value| {
        let mut batch = ports.batch();
        batch.set_custom_field_value(row(d, &owner, json), None);
        ports.commit(batch).await
    };
    write(&erp, customer(1), json!("A-1001")).await.unwrap();
    assert_eq!(
        write(&erp, customer(2), json!("a-1001")).await,
        Err(RepoError::Duplicate(keys::CUSTOM_FIELD_UNIQUE_VALUE))
    );
    // The same owner rewriting it, or another definition, doesn't clash.
    write(&erp, customer(1), json!("A-1001")).await.unwrap();
    write(&other, customer(2), json!("A-1001")).await.unwrap();
    let found = ports
        .custom_field_values()
        .find_unique(erp.id(), "a-1001")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.owner, customer(1).id());
    assert_eq!(
        ports.custom_field_values().find_unique(erp.id(), "b").await,
        Ok(None)
    );
}

async fn value_batch<S>(ports: &S)
where
    S: HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasOutbox
        + HasBatch<Batch: StageCustomFieldValues + StageEvents>,
{
    let tier = setup(
        ports,
        definition(1, OwnerType::Customer, "custom", "tier", "text"),
    )
    .await;
    let before = events(ports).await.len();
    let mut batch = ports.batch();
    batch.set_custom_field_value(row(&tier, &customer(1), json!("gold")), None);
    batch.set_custom_field_value(
        row(&tier, &customer(2), json!("silver")),
        Some(Version::new(3)),
    );
    batch.record_events(
        fields_updated(
            &customer(1),
            vec![FieldChange::value(
                "custom.tier",
                Some(json!("gold")),
                false,
            )],
        )
        .into_iter()
        .collect(),
    );
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(
        ports
            .custom_field_values()
            .get_many(&[customer(1).id()], None)
            .await,
        Ok(vec![])
    );
    assert_eq!(events(ports).await.len(), before);

    // A value of a definition that doesn't exist is refused.
    let missing = definition(9, OwnerType::Customer, "custom", "gone", "text");
    let mut batch = ports.batch();
    batch.set_custom_field_value(row(&missing, &customer(1), json!("x")), None);
    assert!(ports.commit(batch).await.is_err());

    let mut batch = ports.batch();
    batch.set_custom_field_value(row(&tier, &customer(1), json!("gold")), None);
    batch.record_events(
        fields_updated(
            &customer(1),
            vec![FieldChange::value(
                "custom.tier",
                Some(json!("gold")),
                false,
            )],
        )
        .into_iter()
        .collect(),
    );
    ports.commit(batch).await.unwrap();
    let written = events(ports).await;
    assert_eq!(written.len(), before + 1);
    assert_eq!(written[before].event_type, "customer.custom_fields_updated");
}

fn tags_change(
    definition: &CustomFieldDefinition,
    owners: Vec<Owner>,
    add: Value,
    remove: Value,
    case_insensitive: bool,
    max_items: u32,
) -> ItemsChange {
    ItemsChange {
        definition: definition.id(),
        key: definition.field_key().to_string(),
        personal: false,
        owners,
        add: add.as_array().cloned().unwrap_or_default(),
        remove: remove.as_array().cloned().unwrap_or_default(),
        case_insensitive,
        min_items: None,
        max_items,
    }
}

async fn list_deltas<S>(ports: &S)
where
    S: HasCustomFieldDefinitions + HasCustomFieldValues + HasOutbox,
{
    let tags = setup(
        ports,
        definition(1, OwnerType::Customer, "custom", "tags", "list.text"),
    )
    .await;
    let values = ports.custom_field_values();
    let before = events(ports).await.len();
    let add = tags_change(
        &tags,
        vec![customer(1), customer(2)],
        json!(["vip"]),
        json!([]),
        false,
        128,
    );
    let ItemsResult::Applied(first) = values.change_items(&add).await.unwrap() else {
        panic!("refused");
    };
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].added, vec![json!("vip")]);
    assert_eq!(first[0].version, Some(Version::new(1)));
    assert_eq!(events(ports).await.len(), before + 2);

    // Again: nothing changes, no event, same version.
    let ItemsResult::Applied(again) = values.change_items(&add).await.unwrap() else {
        panic!("refused");
    };
    assert!(
        again
            .iter()
            .all(|o| o.added.is_empty() && o.removed.is_empty())
    );
    assert_eq!(again[0].version, Some(Version::new(1)));
    assert_eq!(events(ports).await.len(), before + 2);

    let more = tags_change(
        &tags,
        vec![customer(1)],
        json!(["new"]),
        json!(["absent"]),
        false,
        128,
    );
    values.change_items(&more).await.unwrap();
    let stored = values.get_many(&[customer(1).id()], None).await.unwrap();
    assert_eq!(stored[0].value, json!(["vip", "new"]));
    assert_eq!(stored[0].version, Version::new(2));

    // Removing the last items removes the value.
    let clear = tags_change(
        &tags,
        vec![customer(2)],
        json!([]),
        json!(["vip"]),
        false,
        128,
    );
    let ItemsResult::Applied(cleared) = values.change_items(&clear).await.unwrap() else {
        panic!("refused");
    };
    assert_eq!(cleared[0].version, None);
    assert_eq!(values.get_many(&[customer(2).id()], None).await, Ok(vec![]));
    let last = events(ports).await.pop().unwrap();
    assert_eq!(
        last.data["changes"],
        json!([{"key": "custom.tags", "added": [], "removed": ["vip"]}])
    );
}

async fn list_deltas_ignore_case<S>(ports: &S)
where
    S: HasCustomFieldDefinitions + HasCustomFieldValues,
{
    let tags = setup(
        ports,
        definition(1, OwnerType::Customer, "custom", "tags", "list.text"),
    )
    .await;
    let values = ports.custom_field_values();
    values
        .change_items(&tags_change(
            &tags,
            vec![customer(1)],
            json!(["VIP"]),
            json!([]),
            true,
            128,
        ))
        .await
        .unwrap();
    let ItemsResult::Applied(outcome) = values
        .change_items(&tags_change(
            &tags,
            vec![customer(1)],
            json!(["vip"]),
            json!([]),
            true,
            128,
        ))
        .await
        .unwrap()
    else {
        panic!("refused");
    };
    assert!(outcome[0].added.is_empty());
    values
        .change_items(&tags_change(
            &tags,
            vec![customer(1)],
            json!([]),
            json!(["Vip"]),
            true,
            128,
        ))
        .await
        .unwrap();
    assert_eq!(values.get_many(&[customer(1).id()], None).await, Ok(vec![]));
}

async fn list_limits<S>(ports: &S)
where
    S: HasCustomFieldDefinitions + HasCustomFieldValues + HasOutbox,
{
    let tags = setup(
        ports,
        definition(1, OwnerType::Customer, "custom", "tags", "list.text"),
    )
    .await;
    let values = ports.custom_field_values();
    values
        .change_items(&tags_change(
            &tags,
            vec![customer(2)],
            json!(["a", "b"]),
            json!([]),
            false,
            2,
        ))
        .await
        .unwrap();
    let before = events(ports).await.len();
    assert_eq!(
        values
            .change_items(&tags_change(
                &tags,
                vec![customer(1), customer(2)],
                json!(["c"]),
                json!([]),
                false,
                2
            ))
            .await,
        Ok(ItemsResult::Refused {
            owner: customer(2).id(),
            problem: ItemsProblem::TooMany { count: 3, max: 2 },
        })
    );
    // Not even the owner with room got it.
    assert_eq!(values.get_many(&[customer(1).id()], None).await, Ok(vec![]));
    assert_eq!(events(ports).await.len(), before);
}

async fn value_reads<S>(ports: &S)
where
    S: HasCustomFieldDefinitions + HasCustomFieldValues + HasBatch<Batch: StageCustomFieldValues>,
{
    let tier = setup(
        ports,
        definition(1, OwnerType::Customer, "custom", "level", "integer"),
    )
    .await;
    let refs = setup(
        ports,
        definition(2, OwnerType::Customer, "custom", "liked", "list.reference"),
    )
    .await;
    let one = setup(
        ports,
        definition(3, OwnerType::Customer, "custom", "best", "reference"),
    )
    .await;
    let product = |n: u128| ProductId::from_uuid(Uuid::from_u128(n)).to_string();
    let mut batch = ports.batch();
    for n in 1..=5u128 {
        batch.set_custom_field_value(row(&tier, &customer(n), json!((n % 2) as i64)), None);
    }
    batch.set_custom_field_value(
        row(&refs, &customer(1), json!([product(10), product(11)])),
        None,
    );
    batch.set_custom_field_value(row(&refs, &customer(2), json!([product(11)])), None);
    batch.set_custom_field_value(row(&one, &customer(3), json!(product(11))), None);
    ports.commit(batch).await.unwrap();
    let values = ports.custom_field_values();

    let limit = PageLimit::new(2).unwrap();
    let first = values.page(tier.id(), None, limit).await.unwrap();
    let owners: Vec<Uuid> = first.items.iter().map(|v| v.owner).collect();
    assert_eq!(owners, [customer(1).id(), customer(2).id()]);
    assert!(first.has_more);
    let last = values
        .page(tier.id(), Some(customer(4).id()), limit)
        .await
        .unwrap();
    assert_eq!(last.items.len(), 1);
    assert!(!last.has_more);

    assert_eq!(
        values.values_in_use(tier.id(), 10).await,
        Ok(vec![
            ValueCount {
                value: json!(1),
                owners: 3
            },
            ValueCount {
                value: json!(0),
                owners: 2
            },
        ])
    );
    assert_eq!(
        values.values_in_use(refs.id(), 1).await,
        Ok(vec![ValueCount {
            value: json!(product(11)),
            owners: 2
        }])
    );

    let mut found: Vec<Uuid> = values
        .referencing(&[refs.id(), one.id()], &product(11))
        .await
        .unwrap()
        .into_iter()
        .map(|v| v.owner)
        .collect();
    found.sort();
    assert_eq!(
        found,
        [customer(1).id(), customer(2).id(), customer(3).id()]
    );
    assert_eq!(
        values.referencing(&[one.id()], &product(10)).await,
        Ok(vec![])
    );

    assert_eq!(values.delete_for_owners(&[customer(1).id()]).await, Ok(2));
    assert_eq!(values.delete_for_owners(&[customer(1).id()]).await, Ok(0));
    assert_eq!(values.get_many(&[customer(1).id()], None).await, Ok(vec![]));
}

/// Runs the scenarios of lists filtered on custom fields (design note 0033, §7):
/// products, customers, orders and categories, as the adapter stores their values.
pub async fn custom_field_filters<S>(make: impl AsyncFn() -> S)
where
    S: HasCustomFieldDefinitions
        + HasProducts
        + HasCustomers
        + HasOrders
        + HasCategories
        + HasBatch<Batch: StageCustomFieldValues>,
{
    scenario("custom field filter: any of, list contains, ranges, exists, combined");
    product_filters(&make().await).await;
    scenario("custom field filter: customers, orders and categories by their values");
    other_owner_filters(&make().await).await;
}

fn on(definition: &CustomFieldDefinition, op: FieldFilterOp) -> FieldFilter {
    FieldFilter {
        definition: definition.id(),
        key: definition.field_key().to_string(),
        op,
    }
}

fn text(t: &str) -> FilterScalar {
    FilterScalar::Text(t.to_string())
}

fn number(n: &str) -> FilterScalar {
    FilterScalar::Number(n.to_string())
}

async fn product_filters<S>(ports: &S)
where
    S: HasCustomFieldDefinitions + HasProducts + HasBatch<Batch: StageCustomFieldValues>,
{
    let products = ports.products();
    for n in 1..=4 {
        products
            .insert(&mut super::products::simple(n, &format!("p{n}")))
            .await
            .unwrap();
    }
    let product = |n: u128| Owner::product(ProductId::from_uuid(Uuid::from_u128(n)));
    let material = setup(
        ports,
        definition(101, OwnerType::Product, "custom", "material", "text"),
    )
    .await;
    let weight = setup(
        ports,
        definition(102, OwnerType::Product, "custom", "weight", "decimal"),
    )
    .await;
    let tags = setup(
        ports,
        definition(103, OwnerType::Product, "custom", "tags", "list.text"),
    )
    .await;
    let mut batch = ports.batch();
    batch.set_custom_field_value(row(&material, &product(1), json!("Wool")), None);
    batch.set_custom_field_value(row(&weight, &product(1), json!("1.50")), None);
    batch.set_custom_field_value(row(&tags, &product(1), json!(["vip", "new"])), None);
    batch.set_custom_field_value(row(&material, &product(2), json!("Cotton")), None);
    batch.set_custom_field_value(row(&weight, &product(2), json!("3")), None);
    batch.set_custom_field_value(row(&tags, &product(2), json!(["sale"])), None);
    batch.set_custom_field_value(row(&weight, &product(3), json!("-2")), None);
    ports.commit(batch).await.unwrap();

    let found = async |fields: Vec<FieldFilter>| -> Vec<u128> {
        let filter = ProductFilter::default().with_fields(fields);
        all_pages(products, &filter, ProductSort::Oldest, 2)
            .await
            .into_iter()
            .map(|p| p.as_uuid().as_u128())
            .collect()
    };
    let any = |texts: &[&str]| FieldFilterOp::AnyOf(texts.iter().map(|t| text(t)).collect());
    assert_eq!(found(vec![on(&material, any(&["wool"]))]).await, [1]);
    assert_eq!(
        found(vec![on(&material, any(&["wool", "cotton"]))]).await,
        [1, 2]
    );
    assert_eq!(
        found(vec![on(&material, any(&["Wool"]))]).await,
        Vec::<u128>::new(),
        "operands come lowercased; values are compared as stored"
    );
    assert_eq!(
        found(vec![on(&weight, FieldFilterOp::AnyOf(vec![number("1.5")]))]).await,
        [1],
        "numbers compare by value: 1.5 is 1.50"
    );
    let contains =
        |items: &[&str]| FieldFilterOp::ContainsAny(items.iter().map(|i| json!(i)).collect());
    assert_eq!(found(vec![on(&tags, contains(&["vip"]))]).await, [1]);
    assert_eq!(
        found(vec![on(&tags, contains(&["sale", "new"]))]).await,
        [1, 2]
    );
    assert_eq!(
        found(vec![on(&tags, contains(&["gone"]))]).await,
        Vec::<u128>::new()
    );
    assert_eq!(
        found(vec![on(&weight, FieldFilterOp::Gte(number("1.5")))]).await,
        [1, 2]
    );
    assert_eq!(
        found(vec![on(&weight, FieldFilterOp::Gt(number("1.5")))]).await,
        [2]
    );
    assert_eq!(
        found(vec![on(&weight, FieldFilterOp::Lt(number("1.5")))]).await,
        [3]
    );
    assert_eq!(
        found(vec![on(&weight, FieldFilterOp::Lte(number("3.00")))]).await,
        [1, 2, 3]
    );
    assert_eq!(
        found(vec![on(&material, FieldFilterOp::Lt(text("d")))]).await,
        [2],
        "text ranges compare bytes"
    );
    assert_eq!(
        found(vec![on(&material, FieldFilterOp::Exists(true))]).await,
        [1, 2]
    );
    assert_eq!(
        found(vec![on(&material, FieldFilterOp::Exists(false))]).await,
        [3, 4]
    );
    assert_eq!(
        found(vec![
            on(&weight, FieldFilterOp::Gte(number("-5"))),
            on(&material, FieldFilterOp::Exists(false)),
        ])
        .await,
        [3],
        "every filter must match"
    );
    assert_eq!(found(vec![]).await, [1, 2, 3, 4]);
}

async fn other_owner_filters<S>(ports: &S)
where
    S: HasCustomFieldDefinitions
        + HasCustomers
        + HasOrders
        + HasCategories
        + HasBatch<Batch: StageCustomFieldValues>,
{
    for n in 1..=2u128 {
        ports
            .customers()
            .insert(&mut super::customers::customer(
                n,
                &format!("c{n}@example.com"),
            ))
            .await
            .unwrap();
        ports
            .orders()
            .insert(&mut super::orders::order(n))
            .await
            .unwrap();
        ports
            .categories()
            .insert(&mut category(n, None, (n - 1) as u32))
            .await
            .unwrap();
    }
    let tier = setup(
        ports,
        definition(201, OwnerType::Customer, "custom", "tier", "text"),
    )
    .await;
    let erp = setup(
        ports,
        definition(202, OwnerType::Order, "custom", "erp_number", "integer"),
    )
    .await;
    let banner = setup(
        ports,
        definition(203, OwnerType::Category, "custom", "banner", "text"),
    )
    .await;
    let mut batch = ports.batch();
    batch.set_custom_field_value(row(&tier, &customer(2), json!("gold")), None);
    batch.set_custom_field_value(
        row(
            &erp,
            &Owner::order(OrderId::from_uuid(Uuid::from_u128(1))),
            json!(1001),
        ),
        None,
    );
    batch.set_custom_field_value(
        row(
            &banner,
            &Owner::category(hypha_core::CategoryId::from_uuid(Uuid::from_u128(1))),
            json!("Summer"),
        ),
        None,
    );
    ports.commit(batch).await.unwrap();

    let gold = CustomerFilter::default()
        .with_fields(vec![on(&tier, FieldFilterOp::AnyOf(vec![text("gold")]))]);
    let ids: Vec<u128> = all_customers(ports.customers(), &gold, CustomerSort::Oldest, 5)
        .await
        .into_iter()
        .map(|c| c.as_uuid().as_u128())
        .collect();
    assert_eq!(ids, [2]);

    let numbered =
        OrderFilter::default().with_fields(vec![on(&erp, FieldFilterOp::Gte(number("1000")))]);
    let ids: Vec<u128> = all_orders(ports.orders(), &numbered, OrderSort::Oldest, 5)
        .await
        .into_iter()
        .map(|o| o.as_uuid().as_u128())
        .collect();
    assert_eq!(ids, [1]);

    let mut ids: Vec<u128> = ports
        .categories()
        .list_with_fields(&[on(&banner, FieldFilterOp::Exists(false))])
        .await
        .unwrap()
        .into_iter()
        .map(|c| c.id().as_uuid().as_u128())
        .collect();
    ids.sort();
    assert_eq!(ids, [2]);
    assert_eq!(
        ports
            .categories()
            .list_with_fields(&[])
            .await
            .map(|c| c.len()),
        Ok(2)
    );
}
