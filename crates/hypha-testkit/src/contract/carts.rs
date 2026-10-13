use hypha_cart::{Cart, CartRepo, Quantity, repo::keys};
use hypha_core::{
    CartId, CountryCode, CustomerId, Email, MarketId, ProductId, RepoError, VariantId, Version,
    Versioned,
};
use uuid::Uuid;

use super::scenario;

/// Runs every `CartRepo` scenario, each on a fresh repo from `make`.
pub async fn cart_repo<R: CartRepo>(make: impl AsyncFn() -> R) {
    scenario("cart: insert stores every field at version 1");
    round_trips(&make().await).await;
    scenario("cart: update stores changed lines and bumps the version");
    update_round_trips(&make().await).await;
    scenario("cart: unique ID, stale and missing updates");
    keys_and_stale(&make().await).await;
}

fn cart(n: u128) -> Cart {
    cart_to(n, "HR")
}

fn cart_to(n: u128, country: &str) -> Cart {
    Cart::new(
        CartId::from_uuid(Uuid::from_u128(n)),
        MarketId::from_uuid(Uuid::from_u128(1000 + n)),
        CountryCode::new(country).unwrap(),
    )
}

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn qty(n: u32) -> Quantity {
    Quantity::new(n).unwrap()
}

async fn round_trips<R: CartRepo>(repo: &R) {
    let mut empty = cart(1);
    repo.insert(&mut empty).await.unwrap();
    assert_eq!(empty.version(), Version::new(1));
    assert_eq!(repo.get(empty.id()).await, Ok(Some(empty)));

    // Line order, quantities, the country and the buyer must survive.
    let mut full = cart_to(2, "DE");
    full.set_customer(Some((
        CustomerId::from_uuid(Uuid::from_u128(7)),
        Email::new("ana@example.hr").unwrap(),
    )));
    full.add_line(pid(1), vid(12), qty(3)).unwrap();
    full.add_line(pid(1), vid(11), qty(1)).unwrap();
    full.add_line(pid(2), vid(21), qty(u32::MAX)).unwrap();
    repo.insert(&mut full).await.unwrap();
    assert_eq!(repo.get(full.id()).await, Ok(Some(full)));

    assert_eq!(
        repo.get(CartId::from_uuid(Uuid::from_u128(99))).await,
        Ok(None)
    );
}

async fn update_round_trips<R: CartRepo>(repo: &R) {
    let mut c = cart(1);
    c.add_line(pid(1), vid(11), qty(1)).unwrap();
    c.add_line(pid(2), vid(21), qty(1)).unwrap();
    repo.insert(&mut c).await.unwrap();

    c.set_quantity(vid(11), qty(5)).unwrap();
    c.remove_line(vid(21));
    c.add_line(pid(3), vid(31), qty(2)).unwrap();
    c.set_email(Some(Email::new("ivo@example.hr").unwrap()));
    repo.update(&mut c).await.unwrap();

    assert_eq!(c.version(), Version::new(2));
    assert_eq!(repo.get(c.id()).await, Ok(Some(c)));
}

async fn keys_and_stale<R: CartRepo>(repo: &R) {
    let mut c = cart(1);
    repo.insert(&mut c).await.unwrap();
    assert_eq!(
        repo.insert(&mut cart(1)).await,
        Err(RepoError::Duplicate(keys::CART_ID))
    );

    // Two writers load the same cart; the second save must not drop the first's line.
    let mut stale = c.clone();
    c.add_line(pid(1), vid(11), qty(1)).unwrap();
    repo.update(&mut c).await.unwrap();
    stale.add_line(pid(2), vid(21), qty(1)).unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(c.id()).await, Ok(Some(c)));

    assert_eq!(
        repo.update(&mut cart(9)).await,
        Err(RepoError::StaleVersion)
    );
}
