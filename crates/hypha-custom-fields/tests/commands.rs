//! Custom field commands over a store in memory.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet},
};

use hypha_core::{
    Actor, ApiKeyId, AppId, AppKey, AuthError, Ctx, CustomFieldDefinitionId, CustomerId, ErrorCode,
    EventHandler, FieldFilterOp, FilterScalar, HasBatch, PageLimit, ProductId, RepoError, Scope,
    StoredEvent, Timestamp, UserId, VariantId, Versioned,
};
use hypha_custom_fields::{
    Access, AdminAccess, CallerKind, Capabilities, CommandError, CustomFieldCleanup,
    DefinitionChange, DefinitionRepo, DefinitionSpec, FieldCaller, FieldKey, FieldType, FieldValue,
    HasCustomFieldDefinitions, HasCustomFieldValues, Namespace, Owner, OwnerType, RawFilterOp,
    Reference, ReferenceTargets, SetCustomFields, StageCustomFieldValues, StorefrontAccess,
    ValueRepo, ValueRow, change_list_items, create_definition, delete_definition, field_filter,
    get_custom_fields, limits::MAX_LISTED_KEYS, move_definition, plan_sync, read_custom_fields,
    set_custom_fields, set_field_values, stage_managed_definitions, sync_managed_definitions,
    update_definition, values_in_use,
};
use hypha_testkit::{InMemoryPorts, SequentialIds};
use pollster::block_on;
use serde_json::{Value, json};
use uuid::Uuid;

fn customer(n: u128) -> Owner {
    Owner::customer(CustomerId::from_uuid(Uuid::from_u128(n)))
}

fn product(n: u128) -> Owner {
    Owner::product(ProductId::from_uuid(Uuid::from_u128(n)))
}

fn staff() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        Scope::ALL.to_vec(),
    )
}

fn spec(owner_type: OwnerType, key: &str, field_type: &str, validations: Value) -> DefinitionSpec {
    DefinitionSpec {
        owner_type,
        key: key.to_string(),
        field_type: FieldType::parse(field_type).unwrap(),
        name: key.to_string(),
        description: None,
        validations: serde_json::from_value(validations).unwrap(),
        access: Access::default(),
        private: false,
        personal: false,
        capabilities: Capabilities::default(),
    }
}

struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    ctx: Ctx,
    merchant: CallerKind,
}

impl World {
    fn new() -> Self {
        Self {
            ports: InMemoryPorts::default(),
            // Definitions get IDs from 1000 on, apart from the owners' small numbers.
            ids: SequentialIds::default(),
            ctx: staff(),
            merchant: CallerKind::Merchant,
        }
    }

    fn caller(&self) -> FieldCaller<'_> {
        FieldCaller::new(&self.ctx, &self.merchant)
    }

    fn define(&self, spec: DefinitionSpec) -> CustomFieldDefinitionId {
        block_on(create_definition(&self.ctx, &self.ports, &self.ids, spec))
            .unwrap()
            .id()
    }

    fn set(&self, owner: &Owner, values: Value) -> Result<BTreeMap<String, Value>, CommandError> {
        self.set_as(&self.caller(), owner, values)
    }

    fn set_as(
        &self,
        caller: &FieldCaller<'_>,
        owner: &Owner,
        values: Value,
    ) -> Result<BTreeMap<String, Value>, CommandError> {
        let input = SetCustomFields {
            values: serde_json::from_value(values).unwrap(),
            if_versions: BTreeMap::new(),
        };
        block_on(set_custom_fields(caller, &self.ports, owner, input))
            .map(|f| f.to_json().into_iter().collect())
    }

    fn data(&self, event_type: &str) -> Vec<Value> {
        self.ports
            .outbox
            .events()
            .into_iter()
            .filter(|e| e.event_type == event_type)
            .map(|e| e.data)
            .collect()
    }
}

fn problem_codes(e: CommandError) -> Vec<(String, &'static str)> {
    match e {
        CommandError::FieldsInvalid(problems) => {
            problems.into_iter().map(|p| (p.path, p.code)).collect()
        }
        other => panic!("not FieldsInvalid: {other:?}"),
    }
}

#[test]
fn creating_a_definition_needs_custom_fields_write() {
    let w = World::new();
    let reader = Ctx::new(Actor::System, vec![Scope::CustomFieldsRead]);
    assert_eq!(
        block_on(create_definition(
            &reader,
            &w.ports,
            &w.ids,
            spec(OwnerType::Product, "material", "text", json!({}))
        ))
        .map(|d| d.id()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::CustomFieldsWrite
        )))
    );
}

#[test]
fn changing_a_definition_needs_custom_fields_write() {
    let w = World::new();
    let id = w.define(spec(OwnerType::Product, "material", "text", json!({})));
    let reader = Ctx::new(Actor::System, vec![Scope::CustomFieldsRead]);
    let missing = Err(CommandError::Auth(AuthError::MissingScope(
        Scope::CustomFieldsWrite,
    )));
    let rename = DefinitionChange {
        name: Some("Fabric".to_string()),
        ..Default::default()
    };
    assert_eq!(
        block_on(update_definition(&reader, &w.ports, id, rename)).map(|_| ()),
        missing
    );
    assert_eq!(
        block_on(move_definition(&reader, &w.ports, id, 0)).map(|_| ()),
        missing
    );
    assert_eq!(block_on(delete_definition(&reader, &w.ports, id)), missing);
}

#[test]
fn staging_managed_definitions_needs_apps_write() {
    let w = World::new();
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    let fields_writer = Ctx::new(Actor::System, vec![Scope::CustomFieldsWrite]);
    let mut batch = w.ports.batch();
    assert_eq!(
        block_on(stage_managed_definitions(
            &fields_writer,
            &w.ports,
            &w.ids,
            &namespace,
            vec![spec(OwnerType::Customer, "member_id", "text", json!({}))],
            &mut batch,
        ))
        .map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::AppsWrite
        )))
    );
}

#[test]
fn definitions_are_appended_to_their_owner_types_form() {
    let w = World::new();
    w.define(spec(OwnerType::Product, "material", "text", json!({})));
    let care = w.define(spec(OwnerType::Product, "care", "text", json!({})));
    let tier = w.define(spec(OwnerType::Customer, "tier", "text", json!({})));
    let get = |id| block_on(w.ports.custom_fields.get(id)).unwrap().unwrap();
    assert_eq!(get(care).position(), 1);
    assert_eq!(get(tier).position(), 0);
    assert_eq!(w.data("custom_field_definition.created").len(), 3);
}

#[test]
fn a_storefront_gets_the_definitions_of_what_it_reads() {
    let w = World::new();
    let shown = |mut s: DefinitionSpec| {
        s.access = Access {
            admin: AdminAccess::ReadWrite,
            storefront: StorefrontAccess::Read,
        };
        s
    };
    let material = w.define(shown(spec(
        OwnerType::Product,
        "material",
        "text",
        json!({}),
    )));
    w.define(spec(OwnerType::Product, "cost", "text", json!({})));
    let weave = w.define(shown(spec(OwnerType::Variant, "weave", "text", json!({}))));
    // Storefronts don't read customers: never theirs, whatever the access says.
    w.define(shown(spec(OwnerType::Customer, "tier", "text", json!({}))));
    let storefront = Ctx::new(
        Actor::ApiKey(ApiKeyId::from_uuid(Uuid::from_u128(9))),
        Scope::STOREFRONT.to_vec(),
    );
    let ids = |owner_type| {
        block_on(hypha_custom_fields::storefront_definitions(
            &storefront,
            &w.ports,
            owner_type,
        ))
        .unwrap()
        .iter()
        .map(|d| d.id())
        .collect::<Vec<_>>()
    };
    assert_eq!(ids(None), vec![material, weave]);
    assert_eq!(ids(Some(OwnerType::Variant)), vec![weave]);
    assert_eq!(ids(Some(OwnerType::Customer)), vec![]);
}

#[test]
fn a_key_is_taken_per_owner_type() {
    let w = World::new();
    w.define(spec(OwnerType::Product, "material", "text", json!({})));
    let again = block_on(create_definition(
        &w.ctx,
        &w.ports,
        &w.ids,
        spec(OwnerType::Product, "material", "integer", json!({})),
    ));
    assert_eq!(
        again.map(|d| d.id()),
        Err(CommandError::KeyTaken {
            owner_type: OwnerType::Product,
            key: "custom.material".to_string()
        })
    );
    w.define(spec(OwnerType::Variant, "material", "text", json!({})));
}

#[test]
fn a_lost_race_for_a_key_reports_it_taken() {
    let w = World::new();
    w.define(spec(OwnerType::Product, "material", "text", json!({})));
    w.ports.custom_fields.hide_next_lookups(1);
    let again = block_on(create_definition(
        &w.ctx,
        &w.ports,
        &w.ids,
        spec(OwnerType::Product, "material", "text", json!({})),
    ));
    assert_eq!(again.unwrap_err().code(), "custom_field_exists");
}

#[test]
fn tightening_is_refused_while_stored_values_break_it() {
    let w = World::new();
    let size = w.define(spec(OwnerType::Customer, "size", "integer", json!({})));
    for n in 1..=3u128 {
        w.set(&customer(n), json!({"custom.size": n as i64 * 10}))
            .unwrap();
    }
    let lower = |max: i64| DefinitionChange {
        validations: Some(serde_json::from_value(json!({"max": max})).unwrap()),
        ..Default::default()
    };
    assert_eq!(
        block_on(update_definition(&w.ctx, &w.ports, size, lower(15))).map(|d| d.id()),
        Err(CommandError::ValuesInvalid {
            count: 2,
            owners: vec![customer(2).prefixed(), customer(3).prefixed()],
        })
    );
    let updated = block_on(update_definition(&w.ctx, &w.ports, size, lower(30))).unwrap();
    assert_eq!(updated.version().value(), 2);
    assert_eq!(
        w.data("custom_field_definition.updated")[0]["fields"],
        json!(["validations"])
    );
}

#[test]
fn turning_unique_on_is_refused_while_owners_share_a_value() {
    let w = World::new();
    let erp = w.define(spec(OwnerType::Customer, "erp", "text", json!({})));
    w.set(&customer(1), json!({"custom.erp": "A"})).unwrap();
    w.set(&customer(2), json!({"custom.erp": "a"})).unwrap();
    let unique = DefinitionChange {
        capabilities: Some(Capabilities {
            filterable: false,
            unique: true,
        }),
        ..Default::default()
    };
    assert_eq!(
        block_on(update_definition(&w.ctx, &w.ports, erp, unique))
            .unwrap_err()
            .code(),
        "custom_field_values_invalid"
    );
}

#[test]
fn managed_definitions_are_read_only_through_the_api() {
    let w = World::new();
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    block_on(sync_managed_definitions(
        &w.ctx,
        &w.ports,
        &w.ids,
        &namespace,
        vec![spec(OwnerType::Customer, "member_id", "text", json!({}))],
    ))
    .unwrap();
    let id = block_on(w.ports.custom_fields.list(None)).unwrap()[0].id();
    assert_eq!(
        block_on(delete_definition(&w.ctx, &w.ports, id)),
        Err(CommandError::Managed("app_erp.member_id".to_string()))
    );
}

#[test]
fn deleting_a_definition_deletes_its_values_and_closes_the_gap() {
    let w = World::new();
    let first = w.define(spec(OwnerType::Customer, "first", "text", json!({})));
    let second = w.define(spec(OwnerType::Customer, "second", "text", json!({})));
    w.set(
        &customer(1),
        json!({"custom.first": "x", "custom.second": "y"}),
    )
    .unwrap();
    block_on(delete_definition(&w.ctx, &w.ports, first)).unwrap();
    assert_eq!(
        block_on(w.ports.custom_fields.get_many(&[customer(1).id()], None))
            .unwrap()
            .len(),
        1
    );
    let left = block_on(w.ports.custom_fields.get(second))
        .unwrap()
        .unwrap();
    assert_eq!(left.position(), 0);
    assert_eq!(w.data("custom_field_definition.deleted").len(), 1);
}

#[test]
fn moving_a_definition_renumbers_its_siblings() {
    let w = World::new();
    let a = w.define(spec(OwnerType::Product, "a_field", "text", json!({})));
    let b = w.define(spec(OwnerType::Product, "b_field", "text", json!({})));
    let c = w.define(spec(OwnerType::Product, "c_field", "text", json!({})));
    block_on(move_definition(&w.ctx, &w.ports, c, 0)).unwrap();
    let position = |id| {
        block_on(w.ports.custom_fields.get(id))
            .unwrap()
            .unwrap()
            .position()
    };
    assert_eq!((position(c), position(a), position(b)), (0, 1, 2));
    // Only the moved one records an event.
    assert_eq!(
        w.data("custom_field_definition.updated"),
        vec![
            json!({"definition": c.to_string(), "owner_type": "product", "namespace": "custom", "key": "c_field", "fields": ["position"]})
        ]
    );
}

#[test]
fn syncing_creates_updates_and_deletes_an_apps_fields() {
    let w = World::new();
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    let sync = |specs| {
        block_on(sync_managed_definitions(
            &w.ctx, &w.ports, &w.ids, &namespace, specs,
        ))
    };
    sync(vec![
        spec(OwnerType::Customer, "member_id", "text", json!({})),
        spec(OwnerType::Customer, "score", "integer", json!({})),
    ])
    .unwrap();
    let mut renamed = spec(OwnerType::Customer, "member_id", "text", json!({}));
    renamed.name = "Member".to_string();
    let plan = sync(vec![
        renamed,
        spec(OwnerType::Order, "ref", "text", json!({})),
    ])
    .unwrap();
    assert_eq!(plan.create.len(), 1);
    assert_eq!(plan.update[0].1, vec!["name"]);
    assert_eq!(plan.delete[0].key, "score");
    let keys: Vec<String> = block_on(w.ports.custom_fields.list_namespace(&namespace))
        .unwrap()
        .iter()
        .map(|d| d.field_key().to_string())
        .collect();
    // By owner type (customers before orders), then position.
    assert_eq!(keys, ["app_erp.member_id", "app_erp.ref"]);
}

#[test]
fn syncing_refuses_a_changed_type() {
    let w = World::new();
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    let existing = {
        block_on(sync_managed_definitions(
            &w.ctx,
            &w.ports,
            &w.ids,
            &namespace,
            vec![spec(OwnerType::Customer, "member_id", "text", json!({}))],
        ))
        .unwrap();
        block_on(w.ports.custom_fields.list(None)).unwrap()
    };
    assert_eq!(
        plan_sync(
            &namespace,
            &existing,
            &[spec(OwnerType::Customer, "member_id", "integer", json!({}))]
        ),
        Err(CommandError::TypeChanged("app_erp.member_id".to_string()))
    );
    assert_eq!(
        plan_sync(&Namespace::Custom, &existing, &[]),
        Err(CommandError::NotManagedNamespace(Namespace::Custom))
    );
}

#[test]
fn setting_fields_merges_and_records_one_event() {
    let w = World::new();
    w.define(spec(OwnerType::Product, "material", "text", json!({})));
    w.define(spec(OwnerType::Product, "care", "text", json!({})));
    w.set(
        &product(1),
        json!({"custom.material": "Wool", "custom.care": "Cold"}),
    )
    .unwrap();
    let after = w.set(&product(1), json!({"custom.care": null})).unwrap();
    assert_eq!(
        after,
        BTreeMap::from([("custom.material".to_string(), json!("Wool"))])
    );
    let events = w.data("product.custom_fields_updated");
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[1]["changes"],
        json!([{"key": "custom.care", "value": null}])
    );
}

#[test]
fn a_write_that_changes_nothing_records_nothing() {
    let w = World::new();
    w.define(spec(OwnerType::Product, "material", "text", json!({})));
    w.set(&product(1), json!({"custom.material": "Wool"}))
        .unwrap();
    w.set(&product(1), json!({"custom.material": "Wool", }))
        .unwrap();
    w.set(&product(1), json!({"custom.other_absent": null}))
        .unwrap_err();
    assert_eq!(w.data("product.custom_fields_updated").len(), 1);
}

#[test]
fn every_problem_is_reported_at_once() {
    let w = World::new();
    w.define(spec(OwnerType::Product, "material", "text", json!({})));
    w.define(spec(
        OwnerType::Product,
        "sizes",
        "list.integer",
        json!({"max": 10}),
    ));
    let result = w.set(
        &product(1),
        json!({
            "custom.material": 5,
            "custom.sizes": [1, 20, 3, 40],
            "custom.nope": "x",
            "Bad": "x",
        }),
    );
    assert_eq!(
        problem_codes(result.unwrap_err()),
        vec![
            ("Bad".to_string(), "invalid_custom_field_key"),
            ("custom.material".to_string(), "invalid_type"),
            ("custom.nope".to_string(), "custom_field_unknown"),
            ("custom.sizes[1]".to_string(), "above_max"),
            ("custom.sizes[3]".to_string(), "above_max"),
        ]
    );
    assert!(w.data("product.custom_fields_updated").is_empty());
}

#[test]
fn at_most_fifty_keys_per_call() {
    let w = World::new();
    let values: serde_json::Map<String, Value> = (0..51)
        .map(|i| (format!("custom.k{i}"), json!("x")))
        .collect();
    assert_eq!(
        w.set(&product(1), Value::Object(values)),
        Err(CommandError::TooManyKeys { max: 50 })
    );
}

#[test]
fn a_stale_version_is_refused() {
    let w = World::new();
    w.define(spec(OwnerType::Product, "material", "text", json!({})));
    w.set(&product(1), json!({"custom.material": "Wool"}))
        .unwrap();
    let write = |version: u64| {
        block_on(set_custom_fields(
            &w.caller(),
            &w.ports,
            &product(1),
            SetCustomFields {
                values: BTreeMap::from([("custom.material".to_string(), json!("Silk"))]),
                if_versions: BTreeMap::from([("custom.material".to_string(), version)]),
            },
        ))
    };
    assert_eq!(
        write(2),
        Err(CommandError::Stale("custom.material".to_string()))
    );
    let fields = write(1).unwrap();
    assert_eq!(fields.versions()["custom.material"], 2);
}

#[test]
fn a_unique_value_is_one_owners() {
    let w = World::new();
    let mut erp = spec(OwnerType::Product, "erp", "text", json!({}));
    erp.capabilities.unique = true;
    w.define(erp);
    w.set(&product(1), json!({"custom.erp": "A-1"})).unwrap();
    assert_eq!(
        problem_codes(
            w.set(&product(2), json!({"custom.erp": "a-1"}))
                .unwrap_err()
        ),
        vec![("custom.erp".to_string(), "custom_field_value_taken")]
    );
    w.set(&product(1), json!({"custom.erp": "A-1"})).unwrap();
}

#[test]
fn references_must_exist() {
    let w = World::new();
    w.define(spec(
        OwnerType::Product,
        "related",
        "list.reference",
        json!({"kind": "product"}),
    ));
    let missing = ProductId::from_uuid(Uuid::from_u128(77)).to_string();
    assert_eq!(
        problem_codes(
            w.set(&product(1), json!({"custom.related": [missing]}))
                .unwrap_err()
        ),
        vec![("custom.related[0]".to_string(), "reference_not_found")]
    );
}

#[test]
fn only_its_app_writes_an_app_field() {
    let w = World::new();
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    let mut member = spec(OwnerType::Customer, "member_id", "text", json!({}));
    member.access = Access {
        admin: AdminAccess::Read,
        storefront: StorefrontAccess::None,
    };
    block_on(sync_managed_definitions(
        &w.ctx,
        &w.ports,
        &w.ids,
        &namespace,
        vec![member],
    ))
    .unwrap();
    assert_eq!(
        problem_codes(
            w.set(&customer(1), json!({"app_erp.member_id": "m1"}))
                .unwrap_err()
        ),
        vec![("app_erp.member_id".to_string(), "custom_field_read_only")]
    );
    let app = Ctx::new(
        Actor::App(AppId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::CustomersWrite],
    );
    let erp = CallerKind::App(AppKey::new("app_erp").unwrap());
    w.set_as(
        &FieldCaller::new(&app, &erp),
        &customer(1),
        json!({"app_erp.member_id": "m1"}),
    )
    .unwrap();
}

#[test]
fn private_and_personal_fields_are_read_as_access_says() {
    let w = World::new();
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    let mut secret = spec(OwnerType::Customer, "secret", "text", json!({}));
    secret.private = true;
    let mut birthday = spec(OwnerType::Customer, "birthday", "date", json!({}));
    birthday.personal = true;
    block_on(sync_managed_definitions(
        &w.ctx,
        &w.ports,
        &w.ids,
        &namespace,
        vec![secret, birthday],
    ))
    .unwrap();
    w.set(
        &customer(1),
        json!({"app_erp.secret": "s", "app_erp.birthday": "1990-05-01"}),
    )
    .unwrap();
    let other = Ctx::new(
        Actor::App(AppId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::CustomersRead],
    );
    let kind = CallerKind::App(AppKey::new("app_other").unwrap());
    let seen = block_on(get_custom_fields(
        &FieldCaller::new(&other, &kind),
        &w.ports,
        &customer(1),
    ))
    .unwrap();
    assert!(seen.entries.is_empty());
    assert_eq!(
        seen.redacted,
        vec![FieldKey::parse("app_erp.birthday").unwrap()]
    );
}

#[test]
fn list_items_are_added_on_many_owners() {
    let w = World::new();
    let tags = w.define(spec(
        OwnerType::Customer,
        "tags",
        "list.text",
        json!({"unique_items": true, "max_items": 2}),
    ));
    let change = |owners: Vec<Owner>, add: Value| {
        block_on(change_list_items(
            &w.caller(),
            &w.ports,
            tags,
            owners,
            add.as_array().cloned().unwrap(),
            vec![],
        ))
    };
    let outcomes = change(vec![customer(1), customer(2)], json!(["VIP"])).unwrap();
    assert_eq!(outcomes.len(), 2);
    change(vec![customer(1)], json!(["vip"])).unwrap();
    assert_eq!(w.data("customer.custom_fields_updated").len(), 2);
    assert_eq!(
        change(vec![customer(1)], json!(["a", "b"])),
        Err(CommandError::TooManyItems {
            owner: customer(1).prefixed(),
            count: 3,
            max: 2
        })
    );
    assert_eq!(
        change(vec![product(1)], json!(["a"])).unwrap_err().code(),
        "owner_type_mismatch"
    );
    assert_eq!(change(vec![], json!(["a"])), Err(CommandError::NoOwners));
}

#[test]
fn list_items_need_a_list_field() {
    let w = World::new();
    let tier = w.define(spec(OwnerType::Customer, "tier", "text", json!({})));
    assert_eq!(
        block_on(change_list_items(
            &w.caller(),
            &w.ports,
            tier,
            vec![customer(1)],
            vec![json!("a")],
            vec![]
        )),
        Err(CommandError::NotAList("custom.tier".to_string()))
    );
}

#[test]
fn one_field_is_set_on_many_owners() {
    let w = World::new();
    let tier = w.define(spec(OwnerType::Customer, "tier", "text", json!({})));
    block_on(set_field_values(
        &w.caller(),
        &w.ports,
        tier,
        vec![
            (customer(1), Some(json!("gold"))),
            (customer(2), Some(json!("gold"))),
        ],
    ))
    .unwrap();
    assert_eq!(w.data("customer.custom_fields_updated").len(), 2);
    let codes = problem_codes(
        block_on(set_field_values(
            &w.caller(),
            &w.ports,
            tier,
            vec![(customer(3), Some(json!(1))), (customer(3), None)],
        ))
        .unwrap_err(),
    );
    assert_eq!(
        codes,
        vec![
            ("custom.tier".to_string(), "invalid_type"),
            ("custom.tier".to_string(), "duplicate_owner"),
        ]
    );
}

#[test]
fn values_in_use_count_owners_for_filterable_fields() {
    let w = World::new();
    let mut tags = spec(OwnerType::Customer, "tags", "list.text", json!({}));
    tags.capabilities.filterable = true;
    let tags = w.define(tags);
    w.set(&customer(1), json!({"custom.tags": ["vip", "eu"]}))
        .unwrap();
    w.set(&customer(2), json!({"custom.tags": ["vip"]}))
        .unwrap();
    let counts = block_on(values_in_use(
        &w.caller(),
        &w.ports,
        tags,
        PageLimit::new(10).unwrap(),
    ))
    .unwrap();
    assert_eq!(counts[0].value, json!("vip"));
    assert_eq!(counts[0].owners, 2);
    let plain = w.define(spec(OwnerType::Customer, "plain", "text", json!({})));
    assert_eq!(
        block_on(values_in_use(
            &w.caller(),
            &w.ports,
            plain,
            PageLimit::DEFAULT
        )),
        Err(CommandError::NotFilterable("custom.plain".to_string()))
    );
}

#[test]
fn filters_are_checked_against_their_definitions() {
    let w = World::new();
    let mut weight = spec(
        OwnerType::Product,
        "weight",
        "measurement",
        json!({"dimension": "weight"}),
    );
    weight.capabilities.filterable = true;
    w.define(weight);
    w.define(spec(OwnerType::Product, "plain", "text", json!({})));
    let definitions = block_on(w.ports.custom_fields.list(Some(OwnerType::Product))).unwrap();
    let filter = field_filter(
        &w.caller(),
        &definitions,
        "custom.weight",
        RawFilterOp::Gte,
        &["1.5kg".to_string()],
    )
    .unwrap();
    assert_eq!(
        filter.op,
        FieldFilterOp::Gte(FilterScalar::Number("1500".to_string()))
    );
    assert_eq!(
        field_filter(
            &w.caller(),
            &definitions,
            "custom.plain",
            RawFilterOp::Eq,
            &["x".to_string()]
        ),
        Err(CommandError::NotFilterable("custom.plain".to_string()))
    );
    assert_eq!(
        field_filter(
            &w.caller(),
            &definitions,
            "custom.nope",
            RawFilterOp::Eq,
            &["x".to_string()]
        ),
        Err(CommandError::Unknown("custom.nope".to_string()))
    );
}

fn event(event_type: &str, data: Value) -> StoredEvent {
    StoredEvent {
        seq: 1,
        id: Uuid::from_u128(1),
        event_type: event_type.to_string(),
        version: 1,
        subject_kind: "product".to_string(),
        subject_id: Uuid::from_u128(1),
        subject_version: None,
        actor: Actor::System,
        occurred_at: Timestamp::from_unix_micros(0),
        data,
    }
}

#[test]
fn the_cleanup_deletes_owners_values_and_references_to_them() {
    let w = World::new();
    let material = w.define(spec(OwnerType::Variant, "material", "text", json!({})));
    let related = w.define(spec(
        OwnerType::Customer,
        "liked",
        "list.reference",
        json!({"kind": "variant"}),
    ));
    let gone = VariantId::from_uuid(Uuid::from_u128(50));
    let kept = VariantId::from_uuid(Uuid::from_u128(51));
    let p = ProductId::from_uuid(Uuid::from_u128(5));
    let defs = block_on(w.ports.custom_fields.list(None)).unwrap();
    let def = |id| defs.iter().find(|d| d.id() == id).unwrap();
    let mut batch = w.ports.batch();
    let row = |d: CustomFieldDefinitionId, owner: Owner, json: Value| {
        let d = def(d);
        ValueRow::new(
            d,
            &owner,
            &FieldValue::parse(d.field_type(), d.validations(), &json).unwrap(),
        )
    };
    batch.set_custom_field_value(row(material, Owner::variant(p, gone), json!("wool")), None);
    batch.set_custom_field_value(
        row(
            related,
            customer(1),
            json!([gone.to_string(), kept.to_string()]),
        ),
        None,
    );
    block_on(w.ports.commit(batch)).unwrap();
    let deleted = event(
        "product.deleted",
        json!({"product": p.to_string(), "handle": "mug", "variants": [gone.to_string()]}),
    );
    assert!(EventHandler::<InMemoryPorts>::wants(
        &CustomFieldCleanup,
        "product.deleted"
    ));
    block_on(CustomFieldCleanup.handle(&w.ports, &deleted)).unwrap();
    assert_eq!(
        block_on(w.ports.custom_fields.get_many(&[gone.as_uuid()], None)),
        Ok(vec![])
    );
    let left = block_on(w.ports.custom_fields.get_many(&[customer(1).id()], None)).unwrap();
    assert_eq!(left[0].value, json!([kept.to_string()]));
    assert_eq!(
        w.data("customer.custom_fields_updated"),
        vec![json!({"customer": customer(1).prefixed(), "changes": [
            {"key": "custom.liked", "added": [], "removed": [gone.to_string()]}
        ]})]
    );
    // Running it again finds nothing to do.
    block_on(CustomFieldCleanup.handle(&w.ports, &deleted)).unwrap();
    assert_eq!(w.data("customer.custom_fields_updated").len(), 1);
}

#[test]
fn custom_fields_need_the_owners_read_scope() {
    let w = World::new();
    let ctx = Ctx::new(Actor::System, vec![Scope::ProductsRead]);
    let kind = CallerKind::Merchant;
    assert_eq!(
        block_on(get_custom_fields(
            &FieldCaller::new(&ctx, &kind),
            &w.ports,
            &customer(1)
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::CustomersRead
        )))
    );
}

#[test]
fn the_storefront_reads_only_storefront_fields() {
    let w = World::new();
    let mut shown = spec(OwnerType::Product, "shown", "text", json!({}));
    shown.access.storefront = StorefrontAccess::Read;
    w.define(shown);
    w.define(spec(OwnerType::Product, "hidden", "text", json!({})));
    w.set(
        &product(1),
        json!({"custom.shown": "a", "custom.hidden": "b"}),
    )
    .unwrap();
    let shop = w.ports.storefront();
    let kind = CallerKind::Storefront;
    let fields = block_on(get_custom_fields(
        &FieldCaller::new(&shop, &kind),
        &w.ports,
        &product(1),
    ))
    .unwrap();
    assert_eq!(
        fields.to_json().into_iter().collect::<BTreeMap<_, _>>(),
        BTreeMap::from([("custom.shown".to_string(), json!("a"))])
    );
}

#[test]
fn definition_changes_keep_identity() {
    let w = World::new();
    let id = w.define(spec(OwnerType::Product, "material", "text", json!({})));
    let renamed = block_on(update_definition(
        &w.ctx,
        &w.ports,
        id,
        DefinitionChange {
            name: Some("Fabric".to_string()),
            ..Default::default()
        },
    ))
    .unwrap();
    assert_eq!(renamed.field_key().to_string(), "custom.material");
    assert_eq!(renamed.name(), "Fabric");
}

#[test]
fn staged_definitions_are_written_only_with_their_batch() {
    let w = World::new();
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    let mut batch = w.ports.batch();
    let plan = block_on(stage_managed_definitions(
        &w.ctx,
        &w.ports,
        &w.ids,
        &namespace,
        vec![spec(OwnerType::Customer, "member_id", "text", json!({}))],
        &mut batch,
    ))
    .unwrap();
    assert_eq!(plan.create.len(), 1);
    assert!(!plan.is_empty());
    assert_eq!(block_on(w.ports.custom_fields.list(None)).unwrap(), vec![]);
    block_on(w.ports.commit(batch)).unwrap();
    let stored = block_on(w.ports.custom_fields.list(None)).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].field_key().to_string(), "app_erp.member_id");
}

#[test]
fn reading_many_owners_lists_personal_keys_whether_held_or_not() {
    let w = World::new();
    let mut birthday = spec(OwnerType::Customer, "birthday", "date", json!({}));
    birthday.personal = true;
    w.define(birthday);
    w.define(spec(OwnerType::Customer, "tier", "text", json!({})));
    w.set(
        &customer(1),
        json!({"custom.birthday": "1990-05-01", "custom.tier": "gold"}),
    )
    .unwrap();
    // A secret key: staff sessions see every field.
    let crm = Ctx::new(
        Actor::ApiKey(ApiKeyId::from_uuid(Uuid::from_u128(2))),
        vec![Scope::CustomersRead],
    );
    let key_kind = CallerKind::Merchant;
    let caller = FieldCaller::new(&crm, &key_kind);
    let owners = [Uuid::from_u128(1), Uuid::from_u128(2)];
    let read = block_on(read_custom_fields(
        &caller,
        &w.ports,
        &[OwnerType::Customer],
        &owners,
        None,
    ))
    .unwrap();
    assert_eq!(
        read.json_of(owners[0]),
        json!({"custom.tier": "gold"}).as_object().unwrap().clone()
    );
    assert!(read.json_of(owners[1]).is_empty());
    // The same for an owner with no value: the list doesn't tell who holds one.
    assert_eq!(
        read.redacted,
        vec![FieldKey::parse("custom.birthday").unwrap()]
    );
}

#[test]
fn reading_only_some_keys_returns_those() {
    let w = World::new();
    w.define(spec(OwnerType::Product, "material", "text", json!({})));
    w.define(spec(OwnerType::Product, "care", "text", json!({})));
    w.set(
        &product(1),
        json!({"custom.material": "wool", "custom.care": "hand wash"}),
    )
    .unwrap();
    let owner = Uuid::from_u128(1);
    let material = FieldKey::parse("custom.material").unwrap();
    let read = block_on(read_custom_fields(
        &w.caller(),
        &w.ports,
        &[OwnerType::Product],
        &[owner],
        Some(&[material.clone(), material]),
    ))
    .unwrap();
    assert_eq!(
        read.json_of(owner),
        json!({"custom.material": "wool"})
            .as_object()
            .unwrap()
            .clone()
    );
    assert!(read.redacted.is_empty());
}

#[test]
fn reading_a_key_the_caller_cannot_see_is_unknown() {
    let w = World::new();
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    let mut secret = spec(OwnerType::Product, "secret", "text", json!({}));
    secret.private = true;
    block_on(sync_managed_definitions(
        &w.ctx,
        &w.ports,
        &w.ids,
        &namespace,
        vec![secret],
    ))
    .unwrap();
    let other = Ctx::new(
        Actor::App(AppId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::ProductsRead],
    );
    let kind = CallerKind::App(AppKey::new("app_other").unwrap());
    let result = block_on(read_custom_fields(
        &FieldCaller::new(&other, &kind),
        &w.ports,
        &[OwnerType::Product],
        &[Uuid::from_u128(1)],
        Some(&[FieldKey::parse("app_erp.secret").unwrap()]),
    ));
    assert_eq!(
        result,
        Err(CommandError::Unknown("app_erp.secret".to_string()))
    );
}

#[test]
fn reading_lists_at_most_ten_keys() {
    let w = World::new();
    let keys: Vec<FieldKey> = (0..=MAX_LISTED_KEYS)
        .map(|n| FieldKey::parse(&format!("custom.k{n}")).unwrap())
        .collect();
    let result = block_on(read_custom_fields(
        &w.caller(),
        &w.ports,
        &[OwnerType::Product],
        &[Uuid::from_u128(1)],
        Some(&keys),
    ));
    assert_eq!(
        result,
        Err(CommandError::TooManyListedKeys {
            max: MAX_LISTED_KEYS
        })
    );
}

#[test]
fn reading_needs_the_owners_read_scope() {
    let w = World::new();
    let catalog = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(2))),
        vec![Scope::ProductsRead],
    );
    let kind = CallerKind::Merchant;
    let result = block_on(read_custom_fields(
        &FieldCaller::new(&catalog, &kind),
        &w.ports,
        &[OwnerType::Customer],
        &[Uuid::from_u128(1)],
        None,
    ));
    assert_eq!(
        result,
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::CustomersRead
        )))
    );
}

#[test]
fn the_cleanup_clears_every_target_of_a_deletion_together() {
    let w = World::new();
    let liked = w.define(spec(
        OwnerType::Customer,
        "liked",
        "list.reference",
        json!({"kind": "variant"}),
    ));
    let favourite = w.define(spec(
        OwnerType::Customer,
        "favourite",
        "reference",
        json!({"kind": "product"}),
    ));
    let p = ProductId::from_uuid(Uuid::from_u128(5));
    let (a, b, kept) = (
        VariantId::from_uuid(Uuid::from_u128(50)),
        VariantId::from_uuid(Uuid::from_u128(51)),
        VariantId::from_uuid(Uuid::from_u128(52)),
    );
    let defs = block_on(w.ports.custom_fields.list(None)).unwrap();
    let row = |id: CustomFieldDefinitionId, json: Value| {
        let d = defs.iter().find(|d| d.id() == id).unwrap();
        ValueRow::new(
            d,
            &customer(1),
            &FieldValue::parse(d.field_type(), d.validations(), &json).unwrap(),
        )
    };
    let mut batch = w.ports.batch();
    batch.set_custom_field_value(
        row(
            liked,
            json!([a.to_string(), kept.to_string(), b.to_string()]),
        ),
        None,
    );
    batch.set_custom_field_value(row(favourite, json!(p.to_string())), None);
    block_on(w.ports.commit(batch)).unwrap();
    let deleted = event(
        "product.deleted",
        json!({"product": p.to_string(), "handle": "mug",
               "variants": [a.to_string(), b.to_string()]}),
    );
    block_on(CustomFieldCleanup.handle(&w.ports, &deleted)).unwrap();
    let left = block_on(w.ports.custom_fields.get_many(&[customer(1).id()], None)).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].value, json!([kept.to_string()]));
    // One event for the owner, with both fields' changes.
    let events = w.data("customer.custom_fields_updated");
    assert_eq!(events.len(), 1);
    let mut changes = events[0]["changes"].as_array().unwrap().clone();
    changes.sort_by_key(|c| c["key"].as_str().unwrap().to_string());
    assert_eq!(
        changes,
        vec![
            json!({"key": "custom.favourite", "value": null}),
            json!({"key": "custom.liked", "added": [], "removed": [a.to_string(), b.to_string()]}),
        ]
    );
}

fn unique_change(unique: bool) -> DefinitionChange {
    DefinitionChange {
        capabilities: Some(Capabilities {
            filterable: false,
            unique,
        }),
        ..Default::default()
    }
}

#[test]
fn turning_unique_on_indexes_the_values_stored_before() {
    let w = World::new();
    let erp = w.define(spec(OwnerType::Customer, "erp", "text", json!({})));
    w.set(&customer(1), json!({"custom.erp": "A-1"})).unwrap();
    block_on(update_definition(
        &w.ctx,
        &w.ports,
        erp,
        unique_change(true),
    ))
    .unwrap();
    assert_eq!(
        block_on(w.ports.custom_fields.find_unique(erp, "a-1"))
            .unwrap()
            .map(|v| (v.owner, v.version.value())),
        Some((customer(1).id(), 1)),
        "indexed, its version kept"
    );
    assert_eq!(
        problem_codes(
            w.set(&customer(2), json!({"custom.erp": "a-1"}))
                .unwrap_err()
        ),
        vec![("custom.erp".to_string(), "custom_field_value_taken")]
    );

    block_on(update_definition(
        &w.ctx,
        &w.ports,
        erp,
        unique_change(false),
    ))
    .unwrap();
    assert_eq!(
        block_on(w.ports.custom_fields.find_unique(erp, "a-1")),
        Ok(None)
    );
    w.set(&customer(2), json!({"custom.erp": "a-1"})).unwrap();
}

#[test]
fn syncing_unique_on_indexes_the_values_stored_before() {
    let w = World::new();
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    let member = |unique: bool| {
        let mut member = spec(OwnerType::Customer, "member_id", "text", json!({}));
        member.capabilities.unique = unique;
        member
    };
    let sync = |specs| {
        block_on(sync_managed_definitions(
            &w.ctx, &w.ports, &w.ids, &namespace, specs,
        ))
    };
    sync(vec![member(false)]).unwrap();
    let id = block_on(w.ports.custom_fields.list(None)).unwrap()[0].id();
    let definition = block_on(w.ports.custom_fields.get(id)).unwrap().unwrap();
    let mut batch = w.ports.batch();
    batch.set_custom_field_value(
        ValueRow::new(
            &definition,
            &customer(1),
            &FieldValue::Text("M-7".to_string()),
        ),
        None,
    );
    block_on(w.ports.commit(batch)).unwrap();
    sync(vec![member(true)]).unwrap();
    assert_eq!(
        block_on(w.ports.custom_fields.find_unique(id, "m-7"))
            .unwrap()
            .map(|v| v.owner),
        Some(customer(1).id())
    );
}

#[test]
fn list_filters_compare_items_ignoring_case() {
    let w = World::new();
    let mut tags = spec(OwnerType::Customer, "tags", "list.text", json!({}));
    tags.capabilities.filterable = true;
    w.define(tags);
    let definitions = block_on(w.ports.custom_fields.list(Some(OwnerType::Customer))).unwrap();
    let filter = field_filter(
        &w.caller(),
        &definitions,
        "custom.tags",
        RawFilterOp::Eq,
        &["VIP".to_string(), "Sale".to_string()],
    )
    .unwrap();
    assert_eq!(
        filter.op,
        FieldFilterOp::ContainsAny(vec![json!("vip"), json!("sale")])
    );
}

#[test]
fn syncing_after_a_deletion_numbers_each_form_densely() {
    let w = World::new();
    w.define(spec(OwnerType::Customer, "m_zero", "text", json!({})));
    w.define(spec(OwnerType::Customer, "m_one", "text", json!({})));
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    let sync = |keys: &[&str]| {
        block_on(sync_managed_definitions(
            &w.ctx,
            &w.ports,
            &w.ids,
            &namespace,
            keys.iter()
                .map(|k| spec(OwnerType::Customer, k, "text", json!({})))
                .collect(),
        ))
    };
    sync(&["a_field", "b_field"]).unwrap();
    sync(&["b_field", "c_field"]).unwrap();
    let form: Vec<(String, u32)> = block_on(w.ports.custom_fields.list(Some(OwnerType::Customer)))
        .unwrap()
        .iter()
        .map(|d| (d.key().to_string(), d.position()))
        .collect();
    assert_eq!(
        form,
        [
            ("m_zero".to_string(), 0),
            ("m_one".to_string(), 1),
            ("b_field".to_string(), 2),
            ("c_field".to_string(), 3),
        ]
    );
}

#[test]
fn a_unique_value_may_move_between_owners_in_one_call() {
    let w = World::new();
    let mut erp = spec(OwnerType::Customer, "erp", "text", json!({}));
    erp.capabilities.unique = true;
    let erp = w.define(erp);
    let set = |values: Vec<(Owner, Option<Value>)>| {
        block_on(set_field_values(&w.caller(), &w.ports, erp, values))
    };
    set(vec![
        (customer(1), Some(json!("A-1"))),
        (customer(2), Some(json!("B-2"))),
    ])
    .unwrap();
    set(vec![(customer(3), Some(json!("A-1"))), (customer(1), None)]).unwrap();
    set(vec![
        (customer(2), Some(json!("A-1"))),
        (customer(3), Some(json!("B-2"))),
    ])
    .unwrap();
    let holder = |unique: &str| {
        block_on(w.ports.custom_fields.find_unique(erp, unique))
            .unwrap()
            .map(|v| v.owner)
    };
    assert_eq!(
        (holder("a-1"), holder("b-2")),
        (Some(customer(2).id()), Some(customer(3).id()))
    );
    // Still refused: a holder that keeps it, or two owners of the call taking one.
    assert_eq!(
        problem_codes(set(vec![(customer(4), Some(json!("a-1")))]).unwrap_err()),
        vec![("custom.erp".to_string(), "custom_field_value_taken")]
    );
    assert_eq!(
        problem_codes(
            set(vec![
                (customer(4), Some(json!("C-3"))),
                (customer(5), Some(json!("c-3"))),
            ])
            .unwrap_err()
        ),
        vec![("custom.erp".to_string(), "custom_field_value_taken")]
    );
    assert_eq!(
        problem_codes(
            set(vec![
                (customer(4), Some(json!("A-1"))),
                (customer(2), Some(json!("A-1"))),
            ])
            .unwrap_err()
        ),
        vec![
            ("custom.erp".to_string(), "custom_field_value_taken"),
            ("custom.erp".to_string(), "custom_field_value_taken"),
        ]
    );
}

/// A write another caller makes, on the same ports.
type Race<'a> = Box<dyn FnOnce(&InMemoryPorts) + 'a>;

/// Ports whose next commit first lets `race` write: another writer getting there
/// between a command's reads and its commit.
struct Racing<'a> {
    ports: &'a InMemoryPorts,
    race: RefCell<Option<Race<'a>>>,
}

impl<'a> Racing<'a> {
    fn new(ports: &'a InMemoryPorts, race: impl FnOnce(&InMemoryPorts) + 'a) -> Self {
        Self {
            ports,
            race: RefCell::new(Some(Box::new(race))),
        }
    }
}

impl HasCustomFieldDefinitions for Racing<'_> {
    type CustomFieldDefinitions =
        <InMemoryPorts as HasCustomFieldDefinitions>::CustomFieldDefinitions;
    fn custom_field_definitions(&self) -> &Self::CustomFieldDefinitions {
        self.ports.custom_field_definitions()
    }
}

impl HasCustomFieldValues for Racing<'_> {
    type CustomFieldValues = <InMemoryPorts as HasCustomFieldValues>::CustomFieldValues;
    fn custom_field_values(&self) -> &Self::CustomFieldValues {
        self.ports.custom_field_values()
    }
}

impl ReferenceTargets for Racing<'_> {
    async fn existing(&self, references: &[Reference]) -> Result<HashSet<Reference>, RepoError> {
        self.ports.existing(references).await
    }
}

impl HasBatch for Racing<'_> {
    type Batch = <InMemoryPorts as HasBatch>::Batch;
    fn batch(&self) -> Self::Batch {
        self.ports.batch()
    }
    async fn commit(&self, batch: Self::Batch) -> Result<(), RepoError> {
        let race = self.race.borrow_mut().take();
        if let Some(race) = race {
            race(self.ports);
        }
        self.ports.commit(batch).await
    }
}

#[test]
fn a_lost_race_names_the_key_whose_version_moved() {
    let w = World::new();
    w.define(spec(OwnerType::Product, "a_field", "text", json!({})));
    let b = w.define(spec(OwnerType::Product, "b_field", "text", json!({})));
    w.set(
        &product(1),
        json!({"custom.a_field": "x", "custom.b_field": "y"}),
    )
    .unwrap();
    let racing = Racing::new(&w.ports, |ports| {
        let definition = block_on(ports.custom_fields.get(b)).unwrap().unwrap();
        let mut batch = ports.batch();
        batch.set_custom_field_value(
            ValueRow::new(&definition, &product(1), &FieldValue::Text("z".to_string())),
            None,
        );
        block_on(ports.commit(batch)).unwrap();
    });
    let result = block_on(set_custom_fields(
        &w.caller(),
        &racing,
        &product(1),
        SetCustomFields {
            values: BTreeMap::from([
                ("custom.a_field".to_string(), json!("x2")),
                ("custom.b_field".to_string(), json!("y2")),
            ]),
            if_versions: BTreeMap::from([
                ("custom.a_field".to_string(), 1),
                ("custom.b_field".to_string(), 1),
            ]),
        },
    ));
    assert_eq!(
        result,
        Err(CommandError::Stale("custom.b_field".to_string()))
    );
}

#[test]
fn a_lost_sync_race_names_the_key_created_meanwhile() {
    let w = World::new();
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    let racing = Racing::new(&w.ports, |ports| {
        let mut zeta = hypha_custom_fields::CustomFieldDefinition::new(
            CustomFieldDefinitionId::from_uuid(Uuid::from_u128(500)),
            Namespace::of_app(AppKey::new("app_erp").unwrap()),
            spec(OwnerType::Customer, "zeta", "text", json!({})),
            0,
        )
        .unwrap();
        block_on(ports.custom_fields.insert(&mut zeta)).unwrap();
    });
    let result = block_on(sync_managed_definitions(
        &w.ctx,
        &racing,
        &w.ids,
        &namespace,
        vec![
            spec(OwnerType::Customer, "alpha", "text", json!({})),
            spec(OwnerType::Customer, "zeta", "text", json!({})),
        ],
    ));
    assert_eq!(
        result,
        Err(CommandError::KeyTaken {
            owner_type: OwnerType::Customer,
            key: "app_erp.zeta".to_string(),
        })
    );
}

#[test]
fn raw_keys_are_quoted_in_messages() {
    let w = World::new();
    let definitions = block_on(w.ports.custom_fields.list(None)).unwrap();
    let unknown = field_filter(
        &w.caller(),
        &definitions,
        "custom.nope ",
        RawFilterOp::Eq,
        &["x".to_string()],
    )
    .unwrap_err();
    assert_eq!(unknown.to_string(), "no custom field \"custom.nope \"");
    assert_eq!(
        CommandError::InvalidFilter("custom.w\n".to_string(), "give a value".to_string())
            .to_string(),
        "\"custom.w\\n\": give a value"
    );
}
