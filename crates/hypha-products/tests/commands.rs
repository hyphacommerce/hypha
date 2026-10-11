use hypha_core::{Actor, AuthError, Ctx, Currency, Money, MoneyError, ProductId, Scope, UserId};
use hypha_products::{
    CommandError, CreateSimpleProduct, Handle, HandleError, ProductError, create_simple_product,
    get_product, repo::ProductRepo,
};
use hypha_testkit::{InMemoryProductRepo, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn mug() -> CreateSimpleProduct {
    CreateSimpleProduct {
        handle: "mug".to_string(),
        title: "Mug".to_string(),
        price: Money::new(1000, Currency::Eur),
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

    assert_eq!(mug_2, Err(CommandError::HandleTaken(handle("mug"))));
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
        price: Money::new(-1, Currency::Eur),
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

    assert_eq!(result, Err(CommandError::NotFound(id)));
}
