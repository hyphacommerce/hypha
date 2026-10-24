use hypha_cart::{Cart, CartRepo, repo::keys};
use hypha_core::{
    CartId, CountryCode, CustomerId, Email, MarketId, OrderId, ProductId, Quantity, RepoError,
    VariantId, Version, Versioned,
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
    scenario("cart: clearing the buyer stores no email and no customer");
    buyer_cleared(&make().await).await;
    scenario("cart: a customer's carts, oldest first, open or completed");
    of_customer(&make().await).await;
    scenario("cart: the marketing opt-in round trips");
    marketing_round_trips(&make().await).await;
    scenario("cart: discount codes round trip in the order entered");
    discount_codes_round_trip(&make().await).await;
    scenario("cart: shipping address, choice and quote round trip, and clear");
    shipping_round_trips(&make().await).await;
    scenario("cart: the payment method round trips on insert and update, and clears");
    payment_method_round_trips(&make().await).await;
}

async fn payment_method_round_trips<R: CartRepo>(repo: &R) {
    let method = |n| Some(hypha_core::PaymentMethodId::from_uuid(Uuid::from_u128(n)));
    let mut c = cart(1);
    c.set_payment_method(method(70)).unwrap();
    repo.insert(&mut c).await.unwrap();
    assert_eq!(repo.get(c.id()).await, Ok(Some(c.clone())));
    c.set_payment_method(method(71)).unwrap();
    repo.update(&mut c).await.unwrap();
    let stored = repo.get(c.id()).await.unwrap().unwrap();
    assert_eq!(stored.payment_method(), method(71));
    c.set_payment_method(None).unwrap();
    repo.update(&mut c).await.unwrap();
    assert_eq!(repo.get(c.id()).await, Ok(Some(c)));
}

fn cart(n: u128) -> Cart {
    cart_to(n, "HR")
}

fn cart_to(n: u128, country: &str) -> Cart {
    Cart::new(
        CartId::from_uuid(Uuid::from_u128(n)),
        MarketId::from_uuid(Uuid::from_u128(1000 + n)),
        CountryCode::new(country).unwrap(),
        // A channel per cart, so a mapping bug can't hide behind equal values.
        hypha_core::ChannelId::from_uuid(Uuid::from_u128(500 + n)),
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
    )))
    .unwrap();
    full.add_line(pid(1), vid(12), qty(3)).unwrap();
    full.add_line(pid(1), vid(11), qty(1)).unwrap();
    full.add_line(pid(2), vid(21), qty(u32::MAX)).unwrap();
    repo.insert(&mut full).await.unwrap();
    assert_eq!(repo.get(full.id()).await, Ok(Some(full.clone())));

    // A checked-out cart keeps its order.
    full.complete(OrderId::from_uuid(Uuid::from_u128(99)))
        .unwrap();
    repo.update(&mut full).await.unwrap();
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
    c.remove_line(vid(21)).unwrap();
    c.add_line(pid(3), vid(31), qty(2)).unwrap();
    c.set_email(Some(Email::new("ivo@example.hr").unwrap()))
        .unwrap();
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

async fn buyer_cleared<R: CartRepo>(repo: &R) {
    let mut c = cart(1);
    let ana = CustomerId::from_uuid(Uuid::from_u128(70));
    c.set_customer(Some((ana, Email::new("ana@example.hr").unwrap())))
        .unwrap();
    repo.insert(&mut c).await.unwrap();

    c.set_customer(None).unwrap();
    c.set_email(None).unwrap();
    repo.update(&mut c).await.unwrap();

    let stored = repo.get(c.id()).await.unwrap().unwrap();
    assert_eq!((stored.customer(), stored.email()), (None, None));
    assert_eq!(stored, c);
}

async fn marketing_round_trips<R: CartRepo>(repo: &R) {
    let mut c = cart(1);
    c.set_marketing_opt_in(true).unwrap();
    repo.insert(&mut c).await.unwrap();
    assert_eq!(repo.get(c.id()).await, Ok(Some(c.clone())));
    assert!(repo.get(c.id()).await.unwrap().unwrap().marketing_opt_in());
    c.set_marketing_opt_in(false).unwrap();
    repo.update(&mut c).await.unwrap();
    assert!(!repo.get(c.id()).await.unwrap().unwrap().marketing_opt_in());
}

async fn discount_codes_round_trip<R: CartRepo>(repo: &R) {
    let mut c = cart(1);
    c.add_discount_code("WELCOME10".to_string()).unwrap();
    c.add_discount_code("SHIP".to_string()).unwrap();
    repo.insert(&mut c).await.unwrap();
    let stored = repo.get(c.id()).await.unwrap().unwrap();
    assert_eq!(stored.discount_codes(), ["WELCOME10", "SHIP"]);
    c.remove_discount_code("WELCOME10").unwrap();
    repo.update(&mut c).await.unwrap();
    assert_eq!(
        repo.get(c.id()).await.unwrap().unwrap().discount_codes(),
        ["SHIP"]
    );
}

async fn of_customer<R: CartRepo>(repo: &R) {
    let ana = CustomerId::from_uuid(Uuid::from_u128(70));
    let ivo = CustomerId::from_uuid(Uuid::from_u128(71));
    let email = |s: &str| Email::new(s).unwrap();
    let mut anas_first = cart(1);
    anas_first
        .set_customer(Some((ana, email("ana@example.hr"))))
        .unwrap();
    let mut ivos = cart(2);
    ivos.set_customer(Some((ivo, email("ivo@example.hr"))))
        .unwrap();
    let mut anas_second = cart(3);
    anas_second
        .set_customer(Some((ana, email("ana@example.hr"))))
        .unwrap();
    anas_second
        .complete(hypha_core::OrderId::from_uuid(Uuid::from_u128(900)))
        .unwrap();
    let mut nobodys = cart(4);
    nobodys.set_email(Some(email("ana@example.hr"))).unwrap();
    for c in [&mut anas_first, &mut ivos, &mut anas_second, &mut nobodys] {
        repo.insert(c).await.unwrap();
    }
    assert_eq!(
        repo.of_customer(ana).await,
        Ok(vec![anas_first, anas_second])
    );
    assert_eq!(repo.of_customer(ivo).await, Ok(vec![ivos]));
    assert_eq!(
        repo.of_customer(CustomerId::from_uuid(Uuid::from_u128(72)))
            .await,
        Ok(vec![])
    );
}

async fn shipping_round_trips<R: CartRepo>(repo: &R) {
    use hypha_cart::{SelectedShipping, StoredQuote};
    use hypha_core::{Address, AddressParts, Currency, Money, Price, ShippingOptionId, Timestamp};
    use hypha_shipping::ProviderKey;

    let full = Address::new(AddressParts {
        first_name: Some("Ana".to_string()),
        last_name: "Horvat".to_string(),
        company: Some("Hypha".to_string()),
        line1: "Ilica 1".to_string(),
        line2: Some("Stan 4".to_string()),
        city: "Zagreb".to_string(),
        province: Some("Grad Zagreb".to_string()),
        postal_code: Some("10000".to_string()),
        country: "HR".to_string(),
        phone: Some("+385 1 234".to_string()),
    })
    .unwrap();
    let option = ShippingOptionId::from_uuid(Uuid::from_u128(77));

    // An address and a built-in choice (no quote).
    let mut c = cart(1);
    c.set_shipping_address(Some(full.clone())).unwrap();
    c.select_shipping(Some(SelectedShipping {
        option,
        quote: None,
    }))
    .unwrap();
    repo.insert(&mut c).await.unwrap();
    assert_eq!(repo.get(c.id()).await, Ok(Some(c.clone())));

    // A provider's quote, every field set and the largest amount.
    let quote = StoredQuote {
        amount: Price::new(Money::new(Money::MAX_AMOUNT, Currency::EUR).unwrap()).unwrap(),
        provider: ProviderKey::new("spore_dhl").unwrap(),
        service: "EXPRESS".to_string(),
        reference: Some("q-1".to_string()),
        quoted_at: Timestamp::from_unix_micros(1_767_225_600_123_456),
    };
    c.select_shipping(Some(SelectedShipping {
        option,
        quote: Some(quote),
    }))
    .unwrap();
    repo.update(&mut c).await.unwrap();
    assert_eq!(repo.get(c.id()).await, Ok(Some(c.clone())));

    // Cleared.
    c.set_shipping_address(None).unwrap();
    repo.update(&mut c).await.unwrap();
    let stored = repo.get(c.id()).await.unwrap().unwrap();
    assert_eq!((stored.shipping_address(), stored.shipping()), (None, None));
    assert_eq!(stored, c);
}
