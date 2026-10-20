//! Concurrent list deltas against a real Postgres (design note 0033, CF6): writers on
//! separate connections add items to the same owner's list, which doesn't exist yet,
//! and every item lands.
//!
//! Needs `DATABASE_URL` like the contract tests; skips itself without it.

use futures_util::future::join_all;
use hypha_core::{CustomFieldDefinitionId, CustomerId, Version};
use hypha_custom_fields::{
    Access, Capabilities, CustomFieldDefinition, DefinitionRepo, DefinitionSpec, FieldType,
    ItemsChange, ItemsResult, Namespace, Owner, OwnerType, RawValidations, ValueRepo,
};
use hypha_store_postgres::{CORE, PgStore, migrate};
use serde_json::json;
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

const SCHEMA: &str = "custom_field_race";

async fn connect(url: &str) -> Client {
    let (client, connection) = hypha_store_postgres::database_config(url)
        .expect("DATABASE_URL")
        .connect(NoTls)
        .await
        .expect("connect to DATABASE_URL");
    tokio::spawn(connection);
    client
        .batch_execute(&format!("SET search_path TO {SCHEMA}"))
        .await
        .expect("set search_path");
    client
}

#[tokio::test]
async fn concurrent_items_added_to_a_new_list_all_land() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping the custom field race test");
        return;
    };
    let mut setup = connect(&url).await;
    setup
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE; CREATE SCHEMA {SCHEMA}; \
             SET search_path TO {SCHEMA};"
        ))
        .await
        .unwrap();
    migrate(&mut setup, &[CORE]).await.unwrap();
    let setup = PgStore::new(setup, clock());

    let mut tags = CustomFieldDefinition::new(
        CustomFieldDefinitionId::from_uuid(Uuid::from_u128(1)),
        Namespace::Custom,
        DefinitionSpec {
            owner_type: OwnerType::Customer,
            key: "tags".to_string(),
            field_type: FieldType::parse("list.text").unwrap(),
            name: "Tags".to_string(),
            description: None,
            validations: RawValidations::default(),
            access: Access::default(),
            private: false,
            personal: false,
            capabilities: Capabilities::default(),
        },
        0,
    )
    .unwrap();
    DefinitionRepo::insert(&setup, &mut tags).await.unwrap();
    let owner = Owner::customer(CustomerId::from_uuid(Uuid::from_u128(2)));

    let mut writers = Vec::new();
    for _ in 0..8 {
        writers.push(PgStore::new(connect(&url).await, clock()));
    }
    let results = join_all(writers.iter().enumerate().map(|(n, store)| {
        let change = ItemsChange {
            definition: tags.id(),
            key: "custom.tags".to_string(),
            personal: false,
            owners: vec![owner],
            add: vec![json!(format!("item-{n}"))],
            remove: vec![],
            case_insensitive: false,
            min_items: None,
            max_items: 128,
        };
        async move { store.change_items(&change).await }
    }))
    .await;
    for r in &results {
        assert!(matches!(r, Ok(ItemsResult::Applied(_))), "{r:?}");
    }

    let stored = setup.get_many(&[owner.id()], None).await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].version, Version::new(8));
    let mut items: Vec<String> = stored[0]
        .value
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.as_str().unwrap().to_string())
        .collect();
    items.sort();
    let expected: Vec<String> = (0..8).map(|n| format!("item-{n}")).collect();
    assert_eq!(items, expected);
}

fn clock() -> std::sync::Arc<hypha_testkit::FixedClock> {
    std::sync::Arc::new(hypha_testkit::FixedClock::default())
}
