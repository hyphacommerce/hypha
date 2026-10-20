//! The category commands (design note 0022): the tree's rules, sibling positions,
//! membership and what the storefront sees.

use hypha_categories::{
    Category, CategoryError, CategoryRepo, CategorySettings, CommandError, CreateCategory,
    UpdateCategory, add_category_products, create_category, delete_category, find_categories,
    get_shown_category_by_handle, list_categories, remove_category_products,
    set_product_categories, shown_categories, update_category,
};
use hypha_core::{
    Actor, AuthError, CategoryId, Ctx, CustomFieldDefinitionId, FieldFilter, FieldFilterOp,
    ProductId, Scope, UserId, Versioned,
};
use hypha_testkit::{InMemoryPorts, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    settings: CategorySettings,
}

impl World {
    fn new() -> Self {
        Self {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
            settings: CategorySettings::default(),
        }
    }

    fn try_create(
        &self,
        name: &str,
        parent: Option<CategoryId>,
        position: Option<u32>,
    ) -> Result<Category, CommandError> {
        let input = CreateCategory {
            name: name.to_string(),
            handle: name.to_lowercase().replace(' ', "-"),
            description: None,
            parent,
            position,
            visible: true,
        };
        block_on(create_category(
            &Ctx::system(),
            &self.ports,
            &self.ids,
            &self.settings,
            input,
        ))
    }

    fn create(&self, name: &str, parent: Option<CategoryId>) -> CategoryId {
        self.try_create(name, parent, None).unwrap().id()
    }

    fn update(&self, id: CategoryId, input: UpdateCategory) -> Result<Category, CommandError> {
        block_on(update_category(
            &Ctx::system(),
            &self.ports,
            &self.settings,
            id,
            input,
        ))
    }

    /// Each child's name, in position order, with its stored position.
    fn children(&self, parent: Option<CategoryId>) -> Vec<(String, u32)> {
        let tree = block_on(list_categories(&Ctx::system(), &self.ports)).unwrap();
        tree.children(parent)
            .iter()
            .map(|c| (c.name().to_string(), c.position()))
            .collect()
    }
}

fn named(names: &[&str]) -> Vec<(String, u32)> {
    names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.to_string(), i as u32))
        .collect()
}

#[test]
fn new_categories_go_at_the_end_or_where_asked_and_siblings_stay_dense() {
    let w = World::new();
    w.create("Clothing", None);
    w.create("Home", None);
    w.try_create("Sale", None, Some(0)).unwrap();
    w.try_create("Gifts", None, Some(99)).unwrap();
    assert_eq!(
        w.children(None),
        named(&["Sale", "Clothing", "Home", "Gifts"])
    );
}

#[test]
fn a_parent_must_exist_and_the_depth_limit_holds() {
    let mut w = World::new();
    w.settings.max_depth = 2;
    let missing = CategoryId::from_uuid(Uuid::from_u128(999));
    assert_eq!(
        w.try_create("Orphan", Some(missing), None),
        Err(CommandError::ParentNotFound(missing))
    );
    let top = w.create("Top", None);
    let mid = w.create("Mid", Some(top));
    assert_eq!(
        w.try_create("Deep", Some(mid), None),
        Err(CommandError::Category(CategoryError::TooDeep { max: 2 }))
    );
}

#[test]
fn handles_are_unique_and_follow_the_product_rule() {
    let w = World::new();
    w.create("Shirts", None);
    assert_eq!(
        w.try_create("Shirts", None, None),
        Err(CommandError::HandleTaken("shirts".to_string()))
    );
    let input = CreateCategory {
        name: "Bad".to_string(),
        handle: "Bad Handle".to_string(),
        description: None,
        parent: None,
        position: None,
        visible: false,
    };
    assert!(matches!(
        block_on(create_category(
            &Ctx::system(),
            &w.ports,
            &w.ids,
            &w.settings,
            input
        )),
        Err(CommandError::Category(CategoryError::Handle(_)))
    ));
}

#[test]
fn moving_renumbers_both_sibling_lists() {
    let w = World::new();
    let clothing = w.create("Clothing", None);
    let shirts = w.create("Shirts", Some(clothing));
    w.create("Trousers", Some(clothing));
    let home = w.create("Home", None);
    w.create("Lamps", Some(home));

    let moved = w
        .update(
            shirts,
            UpdateCategory {
                parent: Some(Some(home)),
                position: Some(0),
                ..UpdateCategory::default()
            },
        )
        .unwrap();
    assert_eq!((moved.parent(), moved.position()), (Some(home), 0));
    assert_eq!(w.children(Some(clothing)), named(&["Trousers"]));
    assert_eq!(w.children(Some(home)), named(&["Shirts", "Lamps"]));

    // Reordering among siblings, then to the top level (at the end).
    w.update(
        shirts,
        UpdateCategory {
            position: Some(5),
            ..UpdateCategory::default()
        },
    )
    .unwrap();
    assert_eq!(w.children(Some(home)), named(&["Lamps", "Shirts"]));
    w.update(
        shirts,
        UpdateCategory {
            parent: Some(None),
            ..UpdateCategory::default()
        },
    )
    .unwrap();
    assert_eq!(w.children(None), named(&["Clothing", "Home", "Shirts"]));
}

#[test]
fn a_move_can_neither_make_a_cycle_nor_break_the_depth_limit() {
    let mut w = World::new();
    let a = w.create("A", None);
    let b = w.create("B", Some(a));
    let c = w.create("C", Some(b));
    assert_eq!(
        w.update(
            a,
            UpdateCategory {
                parent: Some(Some(c)),
                ..UpdateCategory::default()
            }
        ),
        Err(CommandError::Category(CategoryError::Cycle {
            category: a,
            parent: c
        }))
    );
    w.settings.max_depth = 3;
    let other = w.create("Other", None);
    // A's subtree spans 3 levels: under Other it would reach 4.
    assert_eq!(
        w.update(
            a,
            UpdateCategory {
                parent: Some(Some(other)),
                ..UpdateCategory::default()
            }
        ),
        Err(CommandError::Category(CategoryError::TooDeep { max: 3 }))
    );
}

#[test]
fn nothing_to_change_writes_nothing() {
    let w = World::new();
    let id = w.create("Shirts", None);
    let before = w.ports.outbox.events().len();
    let same = UpdateCategory {
        name: Some("Shirts".to_string()),
        position: Some(0),
        ..UpdateCategory::default()
    };
    let unchanged = w.update(id, same).unwrap();
    assert_eq!(unchanged.version(), hypha_core::Version::new(1));
    assert_eq!(w.ports.outbox.events().len(), before);
}

#[test]
fn deleting_refuses_a_parent_and_closes_the_gap() {
    let w = World::new();
    let ctx = Ctx::system();
    let clothing = w.create("Clothing", None);
    w.create("Shirts", Some(clothing));
    let home = w.create("Home", None);
    let sale = w.create("Sale", None);
    block_on(add_category_products(&ctx, &w.ports, sale, vec![pid(1)])).unwrap();

    assert_eq!(
        block_on(delete_category(&ctx, &w.ports, clothing)),
        Err(CommandError::HasChildren(clothing))
    );
    block_on(delete_category(&ctx, &w.ports, home)).unwrap();
    assert_eq!(w.children(None), named(&["Clothing", "Sale"]));

    // Its membership goes with it, named in the event; the products stay.
    block_on(delete_category(&ctx, &w.ports, sale)).unwrap();
    let deleted = w.ports.outbox.events().pop().unwrap();
    assert_eq!(deleted.event_type, "category.deleted");
    assert_eq!(deleted.data["products"], serde_json::json!([pid(1)]));
    assert_eq!(
        block_on(w.ports.categories.products_in(&[sale])),
        Ok(vec![])
    );
}

#[test]
fn membership_changes_record_only_what_changed() {
    let w = World::new();
    let shirts = w.create("Shirts", None);
    let ctx = Ctx::system();
    block_on(add_category_products(
        &ctx,
        &w.ports,
        shirts,
        vec![pid(1), pid(2), pid(1)],
    ))
    .unwrap();
    block_on(add_category_products(&ctx, &w.ports, shirts, vec![pid(2)])).unwrap();
    block_on(remove_category_products(
        &ctx,
        &w.ports,
        shirts,
        vec![pid(1), pid(9)],
    ))
    .unwrap();
    let membership: Vec<(String, serde_json::Value)> = w
        .ports
        .outbox
        .events()
        .into_iter()
        .filter(|e| e.event_type.starts_with("category.products"))
        .map(|e| (e.event_type, e.data["products"].clone()))
        .collect();
    assert_eq!(
        membership,
        vec![
            (
                "category.products_added".to_string(),
                serde_json::json!([pid(1), pid(2)])
            ),
            (
                "category.products_removed".to_string(),
                serde_json::json!([pid(1)])
            ),
        ]
    );
}

#[test]
fn a_products_categories_are_replaced_and_must_exist() {
    let w = World::new();
    let shirts = w.create("Shirts", None);
    let sale = w.create("Sale", None);
    let ctx = Ctx::system();
    assert_eq!(
        block_on(set_product_categories(
            &ctx,
            &w.ports,
            pid(1),
            vec![sale, shirts]
        )),
        Ok({
            let mut both = vec![shirts, sale];
            both.sort();
            both
        })
    );
    assert_eq!(
        block_on(set_product_categories(&ctx, &w.ports, pid(1), vec![sale])),
        Ok(vec![sale])
    );
    let missing = CategoryId::from_uuid(Uuid::from_u128(999));
    assert_eq!(
        block_on(set_product_categories(
            &ctx,
            &w.ports,
            pid(1),
            vec![missing]
        )),
        Err(CommandError::CategoryNotFound(missing))
    );
}

#[test]
fn the_storefront_never_sees_a_hidden_category_or_anything_under_it() {
    let w = World::new();
    let clothing = w.create("Clothing", None);
    w.create("Shirts", Some(clothing));
    w.create("Home", None);
    w.update(
        clothing,
        UpdateCategory {
            visible: Some(false),
            ..UpdateCategory::default()
        },
    )
    .unwrap();
    let shopper = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::ProductsRead],
    );
    let tree = block_on(shown_categories(&shopper, &w.ports)).unwrap();
    let top: Vec<&str> = tree.children(None).iter().map(|c| c.name()).collect();
    assert_eq!(top, ["Home"]);
    assert_eq!(
        block_on(get_shown_category_by_handle(&shopper, &w.ports, "shirts")),
        Err(CommandError::HandleNotFound("shirts".to_string()))
    );
    let home = block_on(get_shown_category_by_handle(&shopper, &w.ports, "home")).unwrap();
    assert_eq!((home.category.name(), home.ancestors.len()), ("Home", 0));
}

#[test]
fn writes_need_products_write() {
    let w = World::new();
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::ProductsRead],
    );
    let input = CreateCategory {
        name: "X".to_string(),
        handle: "x".to_string(),
        description: None,
        parent: None,
        position: None,
        visible: true,
    };
    assert_eq!(
        block_on(create_category(
            &reader,
            &w.ports,
            &w.ids,
            &w.settings,
            input
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsWrite
        )))
    );
}

/// A filter every category matches: no one holds this field.
fn nobody_holds() -> Vec<FieldFilter> {
    vec![FieldFilter {
        definition: CustomFieldDefinitionId::from_uuid(Uuid::from_u128(7)),
        key: "custom.banner".to_string(),
        op: FieldFilterOp::Exists(false),
    }]
}

#[test]
fn categories_found_by_fields_come_in_tree_order() {
    let w = World::new();
    let home = w.create("Home", None);
    let garden = w.create("Garden", None);
    w.create("Lighting", Some(home));
    w.create("Kitchen", Some(home));
    w.create("Tools", Some(garden));
    let found = block_on(find_categories(&Ctx::system(), &w.ports, &nobody_holds())).unwrap();
    let names: Vec<&str> = found.iter().map(|c| c.name()).collect();
    assert_eq!(names, ["Home", "Lighting", "Kitchen", "Garden", "Tools"]);
}

#[test]
fn finding_categories_needs_products_read() {
    let w = World::new();
    let nobody = Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), vec![]);
    assert_eq!(
        block_on(find_categories(&nobody, &w.ports, &nobody_holds())),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsRead
        )))
    );
}
