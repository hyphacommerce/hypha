use hypha_core::{
    Address, AddressId, AddressParts, CountryCode, CustomerId, Email, RepoError, Version, Versioned,
};
use hypha_customers::{Customer, CustomerRepo, repo::keys};
use uuid::Uuid;

use super::scenario;

// ---------- fixtures ----------

fn customer_id(n: u128) -> CustomerId {
    CustomerId::from_uuid(Uuid::from_u128(n))
}

fn addr_id(n: u128) -> AddressId {
    AddressId::from_uuid(Uuid::from_u128(n))
}

fn email(s: &str) -> Email {
    Email::new(s).unwrap()
}

fn customer(n: u128, mail: &str) -> Customer {
    Customer::new(customer_id(n), email(mail))
}

/// Every address field filled in.
fn full_address(city: &str) -> Address {
    Address::new(AddressParts {
        first_name: Some("Ana".to_string()),
        last_name: "Horvat".to_string(),
        company: Some("Hypha d.o.o.".to_string()),
        line1: "Ilica 1".to_string(),
        line2: Some("2. kat".to_string()),
        city: city.to_string(),
        province: Some("Grad Zagreb".to_string()),
        postal_code: Some("10000".to_string()),
        country: CountryCode::new("HR").unwrap(),
        phone: Some("+385 1 234 5678".to_string()),
    })
    .unwrap()
}

/// Only the required address fields.
fn bare_address(city: &str) -> Address {
    Address::new(AddressParts {
        first_name: None,
        last_name: "Kovač".to_string(),
        company: None,
        line1: "Riva 2".to_string(),
        line2: None,
        city: city.to_string(),
        province: None,
        postal_code: None,
        country: CountryCode::new("HR").unwrap(),
        phone: None,
    })
    .unwrap()
}

/// Runs every `CustomerRepo` scenario, each on a fresh repo from `make`.
pub async fn customer_repo<R: CustomerRepo>(make: impl AsyncFn() -> R) {
    scenario("customer: insert stores every field at version 1");
    round_trips(&make().await).await;
    scenario("customer: update stores every field and bumps the version");
    update(&make().await).await;
    scenario("customer: unique ID and one customer per email");
    keys(&make().await).await;
    scenario("customer: stale and missing updates fail");
    stale(&make().await).await;
}

async fn round_trips<R: CustomerRepo>(repo: &R) {
    // Address order, both kinds of address, and different defaults must survive.
    let mut c = customer(1, "ana@example.hr");
    c.set_first_name(Some("Ana".to_string())).unwrap();
    c.set_last_name(Some("Horvat".to_string())).unwrap();
    c.set_phone(Some("+385 91 000 0000".to_string())).unwrap();
    c.add_address(addr_id(2), bare_address("Split"), false, true)
        .unwrap();
    c.add_address(addr_id(1), full_address("Zagreb"), true, false)
        .unwrap();
    repo.insert(&mut c).await.unwrap();
    assert_eq!(c.version(), Version::new(1));
    assert_eq!(repo.get(c.id()).await, Ok(Some(c.clone())));
    assert_eq!(repo.get_by_email(c.email()).await, Ok(Some(c)));

    let mut bare = customer(2, "bare@example.hr");
    repo.insert(&mut bare).await.unwrap();
    assert_eq!(repo.get(bare.id()).await, Ok(Some(bare)));

    assert_eq!(repo.get(customer_id(99)).await, Ok(None));
    assert_eq!(
        repo.get_by_email(&email("nobody@example.hr")).await,
        Ok(None)
    );
}

async fn update<R: CustomerRepo>(repo: &R) {
    let mut c = customer(1, "ana@example.hr");
    c.set_first_name(Some("Ana".to_string())).unwrap();
    c.add_address(addr_id(1), full_address("Zagreb"), true, true)
        .unwrap();
    repo.insert(&mut c).await.unwrap();

    c.set_email(email("ana.horvat@example.hr"));
    c.set_first_name(None).unwrap();
    c.set_last_name(Some("Horvat".to_string())).unwrap();
    c.remove_address(addr_id(1)).unwrap();
    c.add_address(addr_id(2), bare_address("Split"), false, true)
        .unwrap();
    repo.update(&mut c).await.unwrap();

    assert_eq!(c.version(), Version::new(2));
    assert_eq!(repo.get(c.id()).await, Ok(Some(c.clone())));
    assert_eq!(repo.get_by_email(&email("ana@example.hr")).await, Ok(None));
    assert_eq!(repo.get_by_email(c.email()).await, Ok(Some(c)));
}

async fn keys<R: CustomerRepo>(repo: &R) {
    repo.insert(&mut customer(1, "ana@example.hr"))
        .await
        .unwrap();
    assert_eq!(
        repo.insert(&mut customer(1, "other@example.hr")).await,
        Err(RepoError::Duplicate(keys::CUSTOMER_ID))
    );
    assert_eq!(
        repo.insert(&mut customer(2, "ana@example.hr")).await,
        Err(RepoError::Duplicate(keys::CUSTOMER_EMAIL))
    );
    assert_eq!(
        repo.get(customer_id(2)).await,
        Ok(None),
        "failed insert stored nothing"
    );

    // Changing an email to one that's taken breaks the key too.
    let mut other = customer(3, "other@example.hr");
    repo.insert(&mut other).await.unwrap();
    other.set_email(email("ana@example.hr"));
    assert_eq!(
        repo.update(&mut other).await,
        Err(RepoError::Duplicate(keys::CUSTOMER_EMAIL))
    );
}

async fn stale<R: CustomerRepo>(repo: &R) {
    let mut c = customer(1, "ana@example.hr");
    repo.insert(&mut c).await.unwrap();
    let mut stale = c.clone();
    c.set_phone(Some("1".to_string())).unwrap();
    repo.update(&mut c).await.unwrap();

    stale.set_phone(Some("2".to_string())).unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(c.id()).await, Ok(Some(c)));

    assert_eq!(
        repo.update(&mut customer(9, "nine@example.hr")).await,
        Err(RepoError::StaleVersion)
    );
}
