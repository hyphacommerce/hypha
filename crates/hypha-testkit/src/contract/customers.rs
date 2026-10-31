use hypha_core::{
    Address, AddressId, AddressParts, CustomerId, Email, Keyset, PageLimit, PageRequest, RepoError,
    Timestamp, Version, Versioned,
};
use hypha_customers::{
    Channel, ConsentChange, ConsentSource, ConsentState, Customer, CustomerFilter, CustomerRepo,
    CustomerSort, OptInLevel, repo::keys,
};
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

pub(crate) fn customer(n: u128, mail: &str) -> Customer {
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
        country: "HR".to_string(),
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
        country: "HR".to_string(),
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
    scenario("customer: list summaries by text, sorted by email or name in pages");
    listed(&make().await).await;
    scenario("customer: an erasure request, the due list and the erased state round trip");
    erasure(&make().await).await;
}

async fn erasure<R: CustomerRepo>(repo: &R) {
    let day = |d: i64| Timestamp::from_unix_micros(1_767_225_600_000_000 + d * 86_400_000_000);
    let mut ana = customer(1, "ana@example.hr");
    ana.set_first_name(Some("Ana".to_string())).unwrap();
    ana.add_address(addr_id(1), full_address("Zagreb"), true, false)
        .unwrap();
    ana.request_erasure(day(2)).unwrap();
    let mut ivo = customer(2, "ivo@example.hr");
    ivo.request_erasure(day(1)).unwrap();
    let mut zed = customer(3, "zed@example.hr");
    zed.request_erasure(day(9)).unwrap();
    let mut done = customer(4, "done@example.hr");
    done.request_erasure(day(1)).unwrap();
    done.erase(day(1));
    for c in [&mut ana, &mut ivo, &mut zed, &mut done] {
        repo.insert(c).await.unwrap();
    }
    assert_eq!(repo.get(ana.id()).await, Ok(Some(ana.clone())));
    assert_eq!(
        repo.due_for_erasure(day(2), 10).await,
        Ok(vec![ivo.id(), ana.id()]),
        "oldest due first, not the erased one, not the one due later"
    );
    assert_eq!(repo.due_for_erasure(day(2), 1).await, Ok(vec![ivo.id()]));
    assert_eq!(repo.due_for_erasure(day(0), 10).await, Ok(vec![]));

    assert!(ana.erase(day(2)));
    repo.update(&mut ana).await.unwrap();
    let stored = repo.get(ana.id()).await.unwrap().unwrap();
    assert_eq!(stored, ana);
    assert!(stored.email().is_placeholder());
    assert_eq!(stored.erased_at(), Some(day(2)));
    assert_eq!(repo.get_by_email(&email("ana@example.hr")).await, Ok(None));
    assert_eq!(
        repo.due_for_erasure(day(9), 10).await,
        Ok(vec![ivo.id(), zed.id()]),
        "the erased one is gone from the list; the rest wait their turn"
    );
}

/// Every page of `sort`'s list under `filter`, `limit` at a time.
pub(crate) async fn all_customers<R: CustomerRepo>(
    repo: &R,
    filter: &CustomerFilter,
    sort: CustomerSort,
    limit: u32,
) -> Vec<CustomerId> {
    let mut found = Vec::new();
    let mut after = None;
    loop {
        let page = repo
            .list(
                filter,
                sort,
                PageRequest {
                    after: after.clone(),
                    limit: PageLimit::new(limit).unwrap(),
                },
            )
            .await
            .unwrap();
        found.extend(page.items.iter().map(|c| c.id));
        match page.items.last() {
            Some(last) if page.has_more => {
                after = Some(Keyset {
                    key: sort.key(last),
                    id: last.id,
                });
            }
            _ => return found,
        }
    }
}

async fn listed<R: CustomerRepo>(repo: &R) {
    // 1: Ana Horvat with two addresses; 2: no name; 3: Ivo Ćorić, a phone; 4: ana
    // again by first name only.
    let mut ana = customer(1, "ana@example.hr");
    ana.set_first_name(Some("Ana".to_string())).unwrap();
    ana.set_last_name(Some("Horvat".to_string())).unwrap();
    ana.add_address(addr_id(1), bare_address("Split"), true, false)
        .unwrap();
    ana.add_address(addr_id(2), bare_address("Zagreb"), false, false)
        .unwrap();
    let mut ivo = customer(3, "Ivo@Example.rs");
    ivo.set_first_name(Some("Ivo".to_string())).unwrap();
    ivo.set_last_name(Some("Ćorić".to_string())).unwrap();
    ivo.set_phone(Some("+381 60 123".to_string())).unwrap();
    let mut other = customer(4, "zed@example.com");
    other.set_first_name(Some("ana".to_string())).unwrap();
    for c in [
        &mut ivo,
        &mut ana,
        &mut customer(2, "b@example.com"),
        &mut other,
    ] {
        repo.insert(c).await.unwrap();
    }

    let first = repo
        .list(
            &CustomerFilter::default(),
            CustomerSort::Oldest,
            PageRequest {
                after: None,
                limit: PageLimit::new(1).unwrap(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        first.items,
        vec![repo.get(customer_id(1)).await.unwrap().unwrap().summary()]
    );
    assert_eq!(first.items[0].address_count, 2);

    let every = CustomerFilter::default();
    let ids = |ns: &[u128]| ns.iter().map(|n| customer_id(*n)).collect::<Vec<_>>();
    for limit in [1, 3, 10] {
        assert_eq!(
            all_customers(repo, &every, CustomerSort::EmailAsc, limit).await,
            ids(&[1, 2, 3, 4]),
            "emails ignoring case"
        );
        assert_eq!(
            all_customers(repo, &every, CustomerSort::NameAsc, limit).await,
            ids(&[2, 4, 1, 3]),
            "no name first, then \"ana \" before \"ana horvat\""
        );
        assert_eq!(
            all_customers(repo, &every, CustomerSort::NameDesc, limit).await,
            ids(&[3, 1, 4, 2])
        );
        assert_eq!(
            all_customers(repo, &every, CustomerSort::Newest, limit).await,
            ids(&[4, 3, 2, 1])
        );
    }
    let text = |q: &str| CustomerFilter::new(Some(q.to_string())).unwrap();
    assert_eq!(
        all_customers(repo, &text("ANA"), CustomerSort::Oldest, 10).await,
        ids(&[1, 4]),
        "email or first name"
    );
    assert_eq!(
        all_customers(repo, &text("ćor"), CustomerSort::Oldest, 10).await,
        ids(&[3]),
        "a last name beyond ASCII"
    );
    assert_eq!(
        all_customers(repo, &text("60 1"), CustomerSort::Oldest, 10).await,
        ids(&[3]),
        "a phone"
    );
    let only = every.clone().only(ids(&[3, 1, 9])).unwrap();
    for limit in [1, 10] {
        assert_eq!(
            all_customers(repo, &only, CustomerSort::EmailAsc, limit).await,
            ids(&[1, 3]),
            "only those IDs; one naming no customer matches nothing"
        );
    }
    assert_eq!(
        all_customers(
            repo,
            &text("ana").only(ids(&[4])).unwrap(),
            CustomerSort::Oldest,
            10
        )
        .await,
        ids(&[4]),
        "with the other filters"
    );
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
    // Both consents, with a source and a time.
    let at = Timestamp::from_unix_micros(1_767_225_600_000_000);
    c.set_marketing_consent(
        Channel::Email,
        ConsentChange {
            state: ConsentState::Subscribed,
            opt_in_level: OptInLevel::Confirmed,
            consented_at: Some(at),
        },
        ConsentSource::App(hypha_core::AppId::from_uuid(Uuid::from_u128(77))),
        at,
    )
    .unwrap();
    c.set_marketing_consent(
        Channel::Sms,
        ConsentChange {
            state: ConsentState::Unsubscribed,
            opt_in_level: OptInLevel::Unknown,
            consented_at: None,
        },
        ConsentSource::Admin,
        at.plus_secs(60),
    )
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

    c.set_marketing_consent(
        Channel::Email,
        ConsentChange {
            state: ConsentState::Pending,
            opt_in_level: OptInLevel::Single,
            consented_at: None,
        },
        ConsentSource::Checkout,
        Timestamp::from_unix_micros(1_767_225_600_000_000),
    )
    .unwrap();
    c.set_email(email("ana.horvat@example.hr")).unwrap();
    assert_eq!(
        c.marketing_consent(Channel::Email).state,
        ConsentState::NotSubscribed,
        "a new email resets email consent"
    );
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
    other.set_email(email("ana@example.hr")).unwrap();
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
