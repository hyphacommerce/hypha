use hypha_core::{Actor, AuthError, Ctx, Scope, StoreId, UserId};
use hypha_stores::{
    CommandError, Store, StoreError, StoreRepo, create_store, ensure_single_store, get_store,
    rename_store,
};
use hypha_testkit::{InMemoryStoreRepo, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn user(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), scopes)
}

#[test]
fn ensure_single_store_creates_the_store_on_first_start() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();

    let store = block_on(ensure_single_store(&Ctx::system(), &repo, &ids, "My store")).unwrap();

    assert_eq!(store.name(), "My store");
    assert_eq!(block_on(repo.list_all()).unwrap(), vec![store]);
}

#[test]
fn ensure_single_store_returns_the_existing_store_on_later_starts() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let first = block_on(ensure_single_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();

    let again = block_on(ensure_single_store(&Ctx::system(), &repo, &ids, "Ignored")).unwrap();

    assert_eq!(again, first);
    assert_eq!(block_on(repo.list_all()).unwrap().len(), 1);
}

#[test]
fn ensure_single_store_refuses_more_than_one_store() {
    let repo = InMemoryStoreRepo::default();
    for n in [1, 2] {
        let mut store = Store::new(StoreId::from_uuid(Uuid::from_u128(n)), "Shop").unwrap();
        block_on(repo.insert(&mut store)).unwrap();
    }

    assert_eq!(
        block_on(ensure_single_store(
            &Ctx::system(),
            &repo,
            &SequentialIds::default(),
            "Shop"
        )),
        Err(CommandError::MultipleStores(2))
    );
}

#[test]
fn create_store_validates_the_name() {
    let repo = InMemoryStoreRepo::default();
    assert_eq!(
        block_on(create_store(
            &Ctx::system(),
            &repo,
            &SequentialIds::default(),
            " "
        )),
        Err(CommandError::Store(StoreError::EmptyName))
    );
    assert_eq!(block_on(repo.list_all()), Ok(vec![]));
}

#[test]
fn rename_store_stores_the_new_name() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();

    block_on(rename_store(&Ctx::system(), &repo, store.id(), "Acme EU")).unwrap();

    let loaded = block_on(get_store(&Ctx::system(), &repo, store.id())).unwrap();
    assert_eq!(loaded.name(), "Acme EU");
}

#[test]
fn get_unknown_store_is_not_found() {
    let id = StoreId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(get_store(&Ctx::system(), &InMemoryStoreRepo::default(), id)),
        Err(CommandError::StoreNotFound(id))
    );
}

#[test]
fn scopes_are_required() {
    let repo = InMemoryStoreRepo::default();
    let ids = SequentialIds::default();
    let store = block_on(create_store(&Ctx::system(), &repo, &ids, "Acme")).unwrap();

    assert_eq!(
        block_on(get_store(&user(vec![]), &repo, store.id())),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::StoresRead
        )))
    );
    assert_eq!(
        block_on(rename_store(
            &user(vec![Scope::StoresRead]),
            &repo,
            store.id(),
            "X"
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::StoresWrite
        )))
    );
}
