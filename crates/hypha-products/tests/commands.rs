use hypha_core::{Actor, AuthError, Ctx, Currency, Money, MoneyError, ProductId, Scope, UserId};
use hypha_products::{
    CommandError, CreateSimpleProduct, Handle, HandleError, ProductError, ProductStatus,
    create_simple_product, get_product, repo::ProductRepo, set_product_status,
};
use hypha_testkit::{InMemoryProductRepo, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn mug() -> CreateSimpleProduct {
    CreateSimpleProduct {
        handle: "mug".to_string(),
        title: "Mug".to_string(),
        price: Money::new(1000, Currency::Eur).unwrap(),
    }
}
fn handle(s: &str) -> Handle {
    Handle::new(s).unwrap()
}

#[test]
fn creates_and_fetches_product() {
    let repo = InMemoryProductRepo::default();
    let ids = SequentialIds::default();
    let ctx = Ctx::system();

    let created = pollster::block_on(create_simple_product(&ctx, &repo, &ids, mug())).unwrap();
    assert_eq!(created.id(), ProductId::from_uuid(Uuid::from_u128(1)));

    let fetched = pollster::block_on(get_product(&ctx, &repo, created.id())).unwrap();
    assert_eq!(fetched, created);
}

#[test]
fn rejects_same_handle_products() {
    let repo = InMemoryProductRepo::default();
    let ids = SequentialIds::default();
    let ctx = Ctx::system();

    let mug_1 = pollster::block_on(create_simple_product(&ctx, &repo, &ids, mug())).unwrap();
    let mug_2 = pollster::block_on(create_simple_product(&ctx, &repo, &ids, mug()));

    assert_eq!(
        mug_2,
        Err(CommandError::HandleTaken {
            handle: handle("mug"),
            product: mug_1.id(),
        })
    );
    assert_eq!(
        block_on(repo.get_by_handle(&handle("mug"))),
        Ok(Some(mug_1))
    );
}

#[test]
fn rejects_missing_write_scope_and_stores_nothing() {
    let repo = InMemoryProductRepo::default();
    let ids = SequentialIds::default();
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::ProductsRead],
    );

    let result = block_on(create_simple_product(&reader, &repo, &ids, mug()));

    assert_eq!(
        result,
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsWrite
        )))
    );
    assert_eq!(block_on(repo.get_by_handle(&handle("mug"))), Ok(None));
}

#[test]
fn rejects_invalid_handle() {
    let repo = InMemoryProductRepo::default();
    let ids = SequentialIds::default();
    let input = CreateSimpleProduct {
        handle: "Bad Handle".to_string(),
        ..mug()
    };

    let result = block_on(create_simple_product(&Ctx::system(), &repo, &ids, input));

    assert_eq!(
        result,
        Err(CommandError::Product(ProductError::InvalidHandle(
            HandleError::InvalidChar('B')
        )))
    );
}

#[test]
fn rejects_negative_price() {
    let repo = InMemoryProductRepo::default();
    let ids = SequentialIds::default();
    let input = CreateSimpleProduct {
        price: Money::new(-1, Currency::Eur).unwrap(),
        ..mug()
    };

    let result = block_on(create_simple_product(&Ctx::system(), &repo, &ids, input));

    assert_eq!(result, Err(CommandError::Money(MoneyError::Negative)));
}

#[test]
fn get_unknown_product_is_not_found() {
    let repo = InMemoryProductRepo::default();
    let id = ProductId::from_uuid(Uuid::from_u128(99));

    let result = block_on(get_product(&Ctx::system(), &repo, id));

    assert_eq!(result, Err(CommandError::ProductNotFound(id)));
}

#[test]
fn set_status_activates_and_stores_product() {
    let repo = InMemoryProductRepo::default();
    let ids = SequentialIds::default();
    let ctx = Ctx::system();
    let created = block_on(create_simple_product(&ctx, &repo, &ids, mug())).unwrap();

    let active = block_on(set_product_status(
        &ctx,
        &repo,
        created.id(),
        ProductStatus::Active,
    ))
    .unwrap();

    assert_eq!(active.status(), ProductStatus::Active);
    assert_eq!(
        block_on(repo.get(created.id()))
            .unwrap()
            .map(|p| p.status()),
        Some(ProductStatus::Active)
    );
}

#[test]
fn set_status_rejects_invalid_transition() {
    let repo = InMemoryProductRepo::default();
    let ids = SequentialIds::default();
    let ctx = Ctx::system();
    let created = block_on(create_simple_product(&ctx, &repo, &ids, mug())).unwrap();

    assert_eq!(
        block_on(set_product_status(
            &ctx,
            &repo,
            created.id(),
            ProductStatus::Archived
        )),
        Err(CommandError::Product(ProductError::InvalidTransition {
            from: ProductStatus::Draft,
            to: ProductStatus::Archived,
        }))
    );
}

#[test]
fn set_status_rejects_unknown_product() {
    let repo = InMemoryProductRepo::default();
    let id = ProductId::from_uuid(Uuid::from_u128(99));

    assert_eq!(
        block_on(set_product_status(
            &Ctx::system(),
            &repo,
            id,
            ProductStatus::Active
        )),
        Err(CommandError::ProductNotFound(id))
    );
}

#[test]
fn handle_taken_by_a_concurrent_insert_is_still_handle_taken() {
    let repo = InMemoryProductRepo::default();
    let ids = SequentialIds::default();
    let first = block_on(create_simple_product(&Ctx::system(), &repo, &ids, mug())).unwrap();

    // The pre-check misses the first mug, as if it was inserted right after.
    repo.hide_next_lookups(1);
    let result = block_on(create_simple_product(&Ctx::system(), &repo, &ids, mug()));

    assert_eq!(
        result,
        Err(CommandError::HandleTaken {
            handle: handle("mug"),
            product: first.id(),
        })
    );
}
