//! The editing commands (design note 0014). The aggregate's rules are unit-tested in
//! `product/edit.rs`; these check scopes, loading, storing and the repository keys.

use hypha_core::{
    Actor, AuthError, Ctx, Currency, Money, MoneyError, OptionValueId, ProductId, Scope, UserId,
    VariantId,
};
use hypha_products::{
    AddVariant, CommandError, CreateSimpleProduct, Handle, MAX_TAGS, OptionDraft, OptionValueDraft,
    Product, ProductError, SetProductOptions, UpdateProduct, UpdateVariant, add_product_variant,
    create_simple_product, remove_product_variant, repo::ProductRepo, set_product_options,
    update_product, update_product_variant,
};
use hypha_testkit::{InMemoryProductRepo, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

struct World {
    repo: InMemoryProductRepo,
    ids: SequentialIds,
    ctx: Ctx,
}

impl World {
    fn new() -> Self {
        Self {
            repo: InMemoryProductRepo::default(),
            ids: SequentialIds::default(),
            ctx: Ctx::system(),
        }
    }

    fn create(&self, handle: &str) -> Product {
        let input = CreateSimpleProduct {
            handle: handle.to_string(),
            title: "Shirt".to_string(),
            price: eur(1000),
        };
        block_on(create_simple_product(
            &self.ctx, &self.repo, &self.ids, input,
        ))
        .unwrap()
    }

    fn stored(&self, id: ProductId) -> Product {
        block_on(self.repo.get(id)).unwrap().unwrap()
    }

    /// A shirt with one option, Size (S, M), and a variant per size: the first variant
    /// (created with the product) is S.
    fn shirt(&self, handle: &str) -> Product {
        let p = self.create(handle);
        let options = SetProductOptions {
            options: vec![new_option("Size", &["S", "M"])],
            remove_variants: vec![],
        };
        let p = block_on(set_product_options(
            &self.ctx,
            &self.repo,
            &self.ids,
            p.id(),
            options,
        ))
        .unwrap();
        let m = p.options()[0].values()[1].id();
        block_on(add_product_variant(
            &self.ctx,
            &self.repo,
            &self.ids,
            p.id(),
            variant(None, vec![m]),
        ))
        .unwrap()
    }

    fn set_sku(
        &self,
        product: ProductId,
        variant: VariantId,
        sku: &str,
    ) -> Result<Product, CommandError> {
        let input = UpdateVariant {
            sku: Some(Some(sku.to_string())),
            ..UpdateVariant::default()
        };
        block_on(update_product_variant(
            &self.ctx, &self.repo, product, variant, input,
        ))
    }
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::Eur).unwrap()
}

fn new_option(name: &str, labels: &[&str]) -> OptionDraft {
    OptionDraft {
        id: None,
        name: name.to_string(),
        values: labels
            .iter()
            .map(|label| OptionValueDraft {
                id: None,
                label: label.to_string(),
            })
            .collect(),
    }
}

fn variant(sku: Option<&str>, selections: Vec<OptionValueId>) -> AddVariant {
    AddVariant {
        sku: sku.map(str::to_string),
        price: eur(1200),
        selections,
        weight: None,
        requires_shipping: true,
    }
}

fn reader() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::ProductsRead],
    )
}

fn missing_write() -> Result<Product, CommandError> {
    Err(CommandError::Auth(AuthError::MissingScope(
        Scope::ProductsWrite,
    )))
}

// ---- update_product ----

#[test]
fn update_product_sets_and_clears_content() {
    let w = World::new();
    let p = w.create("shirt");
    let input = UpdateProduct {
        description: Some(Some("<p>Soft</p><script>x()</script>".to_string())),
        vendor: Some(Some(" Acme ".to_string())),
        product_type: Some(Some("Shirt".to_string())),
        tags: Some(vec!["Summer".to_string(), "summer".to_string()]),
        ..UpdateProduct::default()
    };
    let updated = block_on(update_product(&w.ctx, &w.repo, p.id(), input)).unwrap();
    assert_eq!(
        (
            updated.description().map(|d| d.as_html()),
            updated.vendor(),
            updated.product_type(),
            updated.tags()
        ),
        (
            Some("<p>Soft</p>"),
            Some("Acme"),
            Some("Shirt"),
            &["Summer".to_string()][..]
        )
    );
    assert_eq!(w.stored(p.id()), updated);

    let clear = UpdateProduct {
        description: Some(None),
        vendor: Some(None),
        tags: Some(vec![]),
        ..UpdateProduct::default()
    };
    let cleared = block_on(update_product(&w.ctx, &w.repo, p.id(), clear)).unwrap();
    assert_eq!(
        (
            cleared.description(),
            cleared.vendor(),
            cleared.product_type(),
            cleared.tags()
        ),
        (None, None, Some("Shirt"), &[][..])
    );
}

#[test]
fn update_product_names_the_content_fields_it_changed() {
    let w = World::new();
    let p = w.create("shirt");
    let input = UpdateProduct {
        vendor: Some(Some("Acme".to_string())),
        tags: Some(vec!["Summer".to_string()]),
        ..UpdateProduct::default()
    };
    block_on(update_product(&w.ctx, &w.repo, p.id(), input)).unwrap();
    let updated = w.repo.outbox().events().pop().unwrap();
    assert_eq!(updated.event_type, "product.updated");
    assert_eq!(
        updated.data["fields"],
        serde_json::json!(["tags", "vendor"])
    );
}

#[test]
fn update_product_refuses_too_many_tags() {
    let w = World::new();
    let p = w.create("shirt");
    let input = UpdateProduct {
        tags: Some((0..=MAX_TAGS).map(|i| format!("t{i}")).collect()),
        ..UpdateProduct::default()
    };
    assert_eq!(
        block_on(update_product(&w.ctx, &w.repo, p.id(), input)),
        Err(CommandError::Product(ProductError::TooManyTags {
            max: MAX_TAGS
        }))
    );
}

#[test]
fn update_product_renames_and_changes_the_handle() {
    let w = World::new();
    let p = w.create("shirt");
    let input = UpdateProduct {
        title: Some("Linen shirt".to_string()),
        handle: Some("linen-shirt".to_string()),
        ..UpdateProduct::default()
    };
    let updated = block_on(update_product(&w.ctx, &w.repo, p.id(), input)).unwrap();
    assert_eq!(
        (updated.title(), updated.handle().as_str()),
        ("Linen shirt", "linen-shirt")
    );
    assert_eq!(w.stored(p.id()), updated);
}

#[test]
fn update_product_keeps_fields_not_given() {
    let w = World::new();
    let p = w.create("shirt");
    let input = UpdateProduct {
        title: Some("Linen shirt".to_string()),
        ..UpdateProduct::default()
    };
    let updated = block_on(update_product(&w.ctx, &w.repo, p.id(), input)).unwrap();
    assert_eq!(updated.handle().as_str(), "shirt");
}

#[test]
fn update_product_refuses_a_taken_handle() {
    let w = World::new();
    let mug = w.create("mug");
    let p = w.create("shirt");
    let input = UpdateProduct {
        handle: Some("mug".to_string()),
        ..UpdateProduct::default()
    };
    assert_eq!(
        block_on(update_product(&w.ctx, &w.repo, p.id(), input)),
        Err(CommandError::HandleTaken {
            handle: Handle::new("mug").unwrap(),
            product: mug.id(),
        })
    );
}

#[test]
fn update_product_keeping_its_own_handle_is_fine() {
    let w = World::new();
    let p = w.create("shirt");
    let input = UpdateProduct {
        handle: Some("shirt".to_string()),
        ..UpdateProduct::default()
    };
    assert!(block_on(update_product(&w.ctx, &w.repo, p.id(), input)).is_ok());
}

#[test]
fn a_handle_taken_by_a_concurrent_write_is_still_handle_taken() {
    let w = World::new();
    let mug = w.create("mug");
    let p = w.create("shirt");
    w.repo.hide_next_lookups(1);
    let input = UpdateProduct {
        handle: Some("mug".to_string()),
        ..UpdateProduct::default()
    };
    assert_eq!(
        block_on(update_product(&w.ctx, &w.repo, p.id(), input)),
        Err(CommandError::HandleTaken {
            handle: Handle::new("mug").unwrap(),
            product: mug.id(),
        })
    );
}

#[test]
fn update_product_needs_products_write() {
    let w = World::new();
    let p = w.create("shirt");
    assert_eq!(
        block_on(update_product(
            &reader(),
            &w.repo,
            p.id(),
            UpdateProduct::default()
        )),
        missing_write()
    );
}

#[test]
fn update_product_of_an_unknown_product_is_not_found() {
    let w = World::new();
    let id = ProductId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(update_product(
            &w.ctx,
            &w.repo,
            id,
            UpdateProduct::default()
        )),
        Err(CommandError::ProductNotFound(id))
    );
}

// ---- options ----

#[test]
fn options_are_stored_with_new_ids() {
    let w = World::new();
    let p = w.shirt("shirt");
    let stored = w.stored(p.id());
    assert_eq!(stored, p);
    assert_eq!(stored.options()[0].name(), "Size");
    assert_eq!(
        stored.variant_title(stored.variants()[0].id()),
        Some("S".to_string())
    );
    assert_eq!(
        stored.variant_title(stored.variants()[1].id()),
        Some("M".to_string())
    );
}

#[test]
fn removing_a_value_with_its_variants_is_one_write() {
    let w = World::new();
    let p = w.shirt("shirt");
    let size = &p.options()[0];
    let m_variant = p.variants()[1].id();
    let keep_s = SetProductOptions {
        options: vec![OptionDraft {
            id: Some(size.id()),
            name: "Size".to_string(),
            values: vec![OptionValueDraft {
                id: Some(size.values()[0].id()),
                label: "S".to_string(),
            }],
        }],
        remove_variants: vec![],
    };
    let refused = block_on(set_product_options(&w.ctx, &w.repo, &w.ids, p.id(), keep_s));
    assert_eq!(
        refused,
        Err(CommandError::Product(ProductError::OptionValueInUse {
            variants: vec![m_variant]
        }))
    );
    assert_eq!(w.stored(p.id()), p);

    let keep_s = SetProductOptions {
        options: vec![OptionDraft {
            id: Some(size.id()),
            name: "Size".to_string(),
            values: vec![OptionValueDraft {
                id: Some(size.values()[0].id()),
                label: "S".to_string(),
            }],
        }],
        remove_variants: vec![m_variant],
    };
    let updated = block_on(set_product_options(&w.ctx, &w.repo, &w.ids, p.id(), keep_s)).unwrap();
    assert_eq!(updated.variants().len(), 1);
    assert_eq!(w.stored(p.id()), updated);
}

#[test]
fn set_options_needs_products_write() {
    let w = World::new();
    let p = w.create("shirt");
    let input = SetProductOptions {
        options: vec![],
        remove_variants: vec![],
    };
    assert_eq!(
        block_on(set_product_options(
            &reader(),
            &w.repo,
            &w.ids,
            p.id(),
            input
        )),
        missing_write()
    );
}

// ---- variants ----

#[test]
fn add_variant_stores_it_with_a_new_id() {
    let w = World::new();
    let p = w.shirt("shirt");
    let added = p.variants()[1].clone();
    assert_eq!(added.base_price().money(), eur(1200));
    assert_eq!(w.stored(p.id()).variant(added.id()), Some(&added));
}

#[test]
fn add_variant_refuses_a_negative_price() {
    let w = World::new();
    let p = w.shirt("shirt");
    let input = AddVariant {
        price: eur(-1),
        ..variant(None, vec![])
    };
    assert_eq!(
        block_on(add_product_variant(&w.ctx, &w.repo, &w.ids, p.id(), input)),
        Err(CommandError::Money(MoneyError::Negative))
    );
}

#[test]
fn add_variant_refuses_another_products_sku() {
    let w = World::new();
    let mug = w.create("mug");
    w.set_sku(mug.id(), mug.variants()[0].id(), "SKU-1")
        .unwrap();
    let p = w.create("shirt");
    let options = SetProductOptions {
        options: vec![new_option("Size", &["S", "M"])],
        remove_variants: vec![],
    };
    let p = block_on(set_product_options(
        &w.ctx,
        &w.repo,
        &w.ids,
        p.id(),
        options,
    ))
    .unwrap();
    let m = p.options()[0].values()[1].id();
    assert_eq!(
        block_on(add_product_variant(
            &w.ctx,
            &w.repo,
            &w.ids,
            p.id(),
            variant(Some("SKU-1"), vec![m])
        )),
        Err(CommandError::SkuTaken {
            sku: "SKU-1".to_string(),
            product: mug.id(),
        })
    );
}

#[test]
fn a_sku_taken_by_a_concurrent_write_is_still_sku_taken() {
    let w = World::new();
    let mug = w.create("mug");
    w.set_sku(mug.id(), mug.variants()[0].id(), "SKU-1")
        .unwrap();
    let p = w.create("shirt");
    w.repo.hide_next_lookups(1);
    assert_eq!(
        w.set_sku(p.id(), p.variants()[0].id(), "SKU-1"),
        Err(CommandError::SkuTaken {
            sku: "SKU-1".to_string(),
            product: mug.id(),
        })
    );
}

#[test]
fn update_variant_changes_price_and_sku() {
    let w = World::new();
    let p = w.shirt("shirt");
    let v = p.variants()[0].id();
    let input = UpdateVariant {
        sku: Some(Some(" SHIRT-S ".to_string())),
        price: Some(eur(1900)),
        ..UpdateVariant::default()
    };
    let updated = block_on(update_product_variant(&w.ctx, &w.repo, p.id(), v, input)).unwrap();
    let stored = w.stored(p.id());
    assert_eq!(stored, updated);
    let v = stored.variant(v).unwrap();
    assert_eq!(
        (v.sku(), v.base_price().money()),
        (Some("SHIRT-S"), eur(1900))
    );
}

#[test]
fn keeping_a_variants_own_sku_is_fine() {
    let w = World::new();
    let p = w.create("mug");
    let v = p.variants()[0].id();
    w.set_sku(p.id(), v, "MUG").unwrap();
    assert!(w.set_sku(p.id(), v, "MUG").is_ok());
}

#[test]
fn update_variant_of_an_unknown_variant_is_not_found() {
    let w = World::new();
    let p = w.create("mug");
    let unknown = VariantId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(update_product_variant(
            &w.ctx,
            &w.repo,
            p.id(),
            unknown,
            UpdateVariant::default()
        )),
        Err(CommandError::Product(ProductError::VariantNotFound(
            unknown
        )))
    );
}

#[test]
fn remove_variant_removes_it_but_never_the_last() {
    let w = World::new();
    let p = w.shirt("shirt");
    let (s, m) = (p.variants()[0].id(), p.variants()[1].id());
    let updated = block_on(remove_product_variant(&w.ctx, &w.repo, p.id(), m)).unwrap();
    assert_eq!(w.stored(p.id()), updated);
    assert_eq!(updated.variants().len(), 1);
    assert_eq!(
        block_on(remove_product_variant(&w.ctx, &w.repo, p.id(), s)),
        Err(CommandError::Product(ProductError::NoVariants))
    );
}

#[test]
fn variant_commands_need_products_write() {
    let w = World::new();
    let p = w.shirt("shirt");
    let v = p.variants()[0].id();
    assert_eq!(
        block_on(add_product_variant(
            &reader(),
            &w.repo,
            &w.ids,
            p.id(),
            variant(None, vec![])
        )),
        missing_write()
    );
    assert_eq!(
        block_on(update_product_variant(
            &reader(),
            &w.repo,
            p.id(),
            v,
            UpdateVariant::default()
        )),
        missing_write()
    );
    assert_eq!(
        block_on(remove_product_variant(&reader(), &w.repo, p.id(), v)),
        missing_write()
    );
}

// ---- events (design note 0016) ----

#[test]
fn each_edit_records_what_changed() {
    let w = World::new();
    let p = w.shirt("shirt");
    let (s, m) = (p.variants()[0].id(), p.variants()[1].id());
    let patch = UpdateVariant {
        sku: None,
        price: Some(eur(1200)),
        selections: None,
        weight: None,
        requires_shipping: None,
    };
    block_on(update_product_variant(&w.ctx, &w.repo, p.id(), s, patch)).unwrap();
    block_on(remove_product_variant(&w.ctx, &w.repo, p.id(), m)).unwrap();

    let events = w.repo.outbox().events();
    let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
    assert_eq!(
        types,
        [
            // `shirt`: created, then options set (S selected for the first variant),
            // then M added.
            "product.created",
            "product.updated",
            "product.variant_updated",
            "product.variant_added",
            "product.variant_updated",
            "product.variant_removed",
        ]
    );
    let price = &events[4];
    assert_eq!(price.data["variant"], s.to_string());
    assert_eq!(price.data["fields"], serde_json::json!(["price"]));
    assert_eq!(events[5].data["variant"], m.to_string());
    // Each write's events carry the version that write gave the product.
    let versions: Vec<u64> = events
        .iter()
        .map(|e| e.subject_version.unwrap().value())
        .collect();
    assert_eq!(versions, [1, 2, 2, 3, 4, 5]);
}

#[test]
fn an_edit_that_changes_nothing_records_nothing() {
    let w = World::new();
    let p = w.create("shirt");
    let input = UpdateProduct {
        title: Some("Shirt".to_string()),
        handle: Some("shirt".to_string()),
        ..UpdateProduct::default()
    };
    block_on(update_product(&w.ctx, &w.repo, p.id(), input)).unwrap();
    assert_eq!(w.repo.outbox().types(), ["product.created"]);
}
