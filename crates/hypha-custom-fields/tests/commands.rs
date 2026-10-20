//! Custom field commands (design note 0033) over a store in memory.

use std::collections::BTreeMap;

use hypha_core::{
    Actor, AppId, AppKey, AuthError, Ctx, CustomFieldDefinitionId, CustomerId, ErrorCode,
    EventHandler, FieldFilterOp, FilterScalar, HasBatch, PageLimit, ProductId, Scope, StoredEvent,
    Timestamp, UserId, VariantId, Versioned,
};
use hypha_custom_fields::{
    Access, AdminAccess, CallerKind, Capabilities, CommandError, CustomFieldCleanup,
    DefinitionChange, DefinitionRepo, DefinitionSpec, FieldCaller, FieldKey, FieldType, FieldValue,
    Namespace, Owner, OwnerType, RawFilterOp, SetCustomFields, StageCustomFieldValues,
    StorefrontAccess, ValueRepo, ValueRow, change_list_items, create_definition, delete_definition,
    field_filter, get_custom_fields, move_definition, plan_sync, set_custom_fields,
    set_field_values, sync_managed_definitions, update_definition, values_in_use,
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
