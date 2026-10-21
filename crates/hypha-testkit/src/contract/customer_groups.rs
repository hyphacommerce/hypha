use std::collections::HashMap;

use hypha_core::{
    CustomerGroupId, CustomerId, Email, Handle, HasBatch, Keyset, PageLimit, PageRequest,
    RepoError, StageEvents, Version, Versioned,
};
use hypha_customers::{
    Customer, CustomerFilter, CustomerGroup, CustomerGroupRepo, CustomerRepo, CustomerSort,
    HasCustomerGroups, HasCustomers, StageCustomerGroups, repo::keys,
};
use uuid::Uuid;

use super::scenario;

fn group_id(n: u128) -> CustomerGroupId {
    CustomerGroupId::from_uuid(Uuid::from_u128(n))
}

fn customer_id(n: u128) -> CustomerId {
    CustomerId::from_uuid(Uuid::from_u128(n))
}

fn group(n: u128, name: &str, handle: &str) -> CustomerGroup {
    CustomerGroup::new(group_id(n), name, Handle::new(handle).unwrap(), None).unwrap()
}

async fn customers<S: HasCustomers>(ports: &S, ns: &[u128]) -> Vec<CustomerId> {
    let mut ids = Vec::new();
    for n in ns {
        let mut c = Customer::new(
            customer_id(*n),
            Email::new(&format!("c{n}@example.hr")).unwrap(),
        );
        ports.customers().insert(&mut c).await.unwrap();
        ids.push(c.id());
    }
    ids
}

/// Runs every `CustomerGroupRepo` scenario (design note 0032, §6), each on fresh ports
/// from `make`: the groups themselves, their keys, membership as sets, the customer
/// list's group filter, and the batch's group writes.
pub async fn customer_group_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasCustomers + HasCustomerGroups + HasBatch,
    S::Batch: StageCustomerGroups + StageEvents,
{
    scenario("customer group: insert and update round trip, listed by name");
    round_trips(&make().await).await;
    scenario("customer group: unique ID, handle and name (ignoring case); stale updates");
    unique_keys(&make().await).await;
    scenario("customer group: membership as sets, paged, and the customers' groups");
    membership(&make().await).await;
    scenario("customer group: the customer list narrows to a group");
    list_filter(&make().await).await;
    scenario("customer group: a batch writes groups and membership with versions");
    batched(&make().await).await;
}

async fn round_trips<S: HasCustomerGroups>(ports: &S) {
    let repo = ports.customer_groups();
    let mut wholesale = group(1, "Wholesale", "wholesale");
    wholesale
        .set_description(Some("Buys in bulk".to_string()))
        .unwrap();
    repo.insert(&mut wholesale).await.unwrap();
    assert_eq!(wholesale.version(), Version::new(1));
    assert_eq!(repo.get(wholesale.id()).await, Ok(Some(wholesale.clone())));
    assert_eq!(
        repo.get_by_handle("wholesale").await,
        Ok(Some(wholesale.clone()))
    );
    assert_eq!(repo.get_by_handle("nope").await, Ok(None));
    assert_eq!(repo.get(group_id(9)).await, Ok(None));

    wholesale.rename("Trade").unwrap();
    wholesale.set_description(None).unwrap();
    repo.update(&mut wholesale).await.unwrap();
    assert_eq!(wholesale.version(), Version::new(2));
    assert_eq!(repo.get(wholesale.id()).await, Ok(Some(wholesale.clone())));

    let mut vip = group(2, "VIP", "vip");
    let mut agencies = group(3, "agencies", "agencies");
    repo.insert(&mut vip).await.unwrap();
    repo.insert(&mut agencies).await.unwrap();
    let names: Vec<String> = repo
        .list_all()
        .await
        .unwrap()
        .iter()
        .map(|g| g.name().to_string())
        .collect();
    assert_eq!(names, ["agencies", "Trade", "VIP"], "by name ignoring case");
}

async fn unique_keys<S: HasCustomerGroups>(ports: &S) {
    let repo = ports.customer_groups();
    repo.insert(&mut group(1, "Wholesale", "wholesale"))
        .await
        .unwrap();
    assert_eq!(
        repo.insert(&mut group(1, "Other", "other")).await,
        Err(RepoError::Duplicate(keys::CUSTOMER_GROUP_ID))
    );
    assert_eq!(
        repo.insert(&mut group(2, "Other", "wholesale")).await,
        Err(RepoError::Duplicate(keys::CUSTOMER_GROUP_HANDLE))
    );
    assert_eq!(
        repo.insert(&mut group(2, "WHOLESALE", "other")).await,
        Err(RepoError::Duplicate(keys::CUSTOMER_GROUP_NAME))
    );
    let mut stale = repo.get(group_id(1)).await.unwrap().unwrap();
    let mut fresh = stale.clone();
    fresh.rename("Trade").unwrap();
    repo.update(&mut fresh).await.unwrap();
    stale.rename("Old").unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    let mut missing = group(7, "Missing", "missing");
    missing.set_version(Version::new(1));
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

async fn membership<S: HasCustomers + HasCustomerGroups>(ports: &S) {
    let repo = ports.customer_groups();
    let [a, b, c] = customers(ports, &[1, 2, 3]).await[..] else {
        unreachable!()
    };
    let mut vip = group(1, "VIP", "vip");
    let mut trade = group(2, "Trade", "trade");
    repo.insert(&mut vip).await.unwrap();
    repo.insert(&mut trade).await.unwrap();

    repo.add_members(vip.id(), &[a, b]).await.unwrap();
    repo.add_members(vip.id(), &[b, c]).await.unwrap();
    repo.add_members(trade.id(), &[c]).await.unwrap();
    assert_eq!(repo.members_in(vip.id()).await, Ok(vec![a, b, c]));
    assert_eq!(repo.members_in(trade.id()).await, Ok(vec![c]));
    assert_eq!(repo.members_in(group_id(9)).await, Ok(vec![]));

    let page = |after: Option<CustomerId>, limit: u32| PageRequest {
        after,
        limit: PageLimit::new(limit).unwrap(),
    };
    let first = repo.member_page(vip.id(), page(None, 2)).await.unwrap();
    assert_eq!(first.items, vec![c, b], "newest first");
    assert!(first.has_more);
    let rest = repo.member_page(vip.id(), page(Some(b), 2)).await.unwrap();
    assert_eq!(rest.items, vec![a]);
    assert!(!rest.has_more);

    let groups = repo.groups_of(&[a, c, customer_id(9)]).await.unwrap();
    let expected: HashMap<CustomerId, Vec<CustomerGroupId>> =
        [(a, vec![vip.id()]), (c, vec![vip.id(), trade.id()])].into();
    assert_eq!(groups, expected);
    assert_eq!(repo.groups_of(&[]).await, Ok(HashMap::new()));

    // A missing group stores no membership.
    assert!(repo.add_members(group_id(9), &[a]).await.is_err());
}

async fn list_filter<S: HasCustomers + HasCustomerGroups>(ports: &S) {
    let [a, _b, c] = customers(ports, &[1, 2, 3]).await[..] else {
        unreachable!()
    };
    let mut vip = group(1, "VIP", "vip");
    ports.customer_groups().insert(&mut vip).await.unwrap();
    ports
        .customer_groups()
        .add_members(vip.id(), &[a, c])
        .await
        .unwrap();
    let page = ports
        .customers()
        .list(
            &CustomerFilter::default().in_group(Some(vip.id())),
            CustomerSort::Oldest,
            PageRequest::<Keyset<CustomerId>> {
                after: None,
                limit: PageLimit::new(10).unwrap(),
            },
        )
        .await
        .unwrap();
    let ids: Vec<CustomerId> = page.items.iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![a, c]);
    let none = ports
        .customers()
        .list(
            &CustomerFilter::default().in_group(Some(group_id(9))),
            CustomerSort::Oldest,
            PageRequest::<Keyset<CustomerId>> {
                after: None,
                limit: PageLimit::new(10).unwrap(),
            },
        )
        .await
        .unwrap();
    assert!(none.items.is_empty());
}

async fn batched<S>(ports: &S)
where
    S: HasCustomers + HasCustomerGroups + HasBatch,
    S::Batch: StageCustomerGroups + StageEvents,
{
    let [a, b] = customers(ports, &[1, 2]).await[..] else {
        unreachable!()
    };
    let mut vip = group(1, "VIP", "vip");
    let mut batch = ports.batch();
    batch.insert_group(&mut vip);
    batch.add_group_members(vip.id(), &[a, b]);
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(vip.version(), Version::new(1));
    assert_eq!(
        ports.customer_groups().get(vip.id()).await,
        Ok(Some(vip.clone()))
    );
    assert_eq!(
        ports.customer_groups().members_in(vip.id()).await,
        Ok(vec![a, b])
    );

    let mut changed = vip.clone();
    changed.rename("Very important").unwrap();
    let mut batch = ports.batch();
    batch.update_group(&mut changed);
    batch.remove_group_members(vip.id(), &[a]);
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(changed.version(), Version::new(2));
    assert_eq!(
        ports.customer_groups().members_in(vip.id()).await,
        Ok(vec![b])
    );

    // A stale group writes nothing, not even the membership staged with it.
    let mut stale = vip;
    stale.rename("Stale").unwrap();
    let mut batch = ports.batch();
    batch.update_group(&mut stale);
    batch.add_group_members(stale.id(), &[a]);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(
        ports.customer_groups().members_in(stale.id()).await,
        Ok(vec![b])
    );
    assert_eq!(
        ports.customer_groups().get(stale.id()).await,
        Ok(Some(changed.clone()))
    );

    // A taken handle writes nothing either.
    let mut twin = group(2, "Twin", "vip");
    let mut batch = ports.batch();
    batch.insert_group(&mut twin);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::CUSTOMER_GROUP_HANDLE))
    );

    // Deleting drops the membership and leaves the customers.
    let mut batch = ports.batch();
    batch.delete_group(changed.id());
    assert_eq!(ports.commit(batch).await, Ok(()));
    assert_eq!(ports.customer_groups().get(changed.id()).await, Ok(None));
    assert_eq!(
        ports.customer_groups().groups_of(&[b]).await,
        Ok(HashMap::new())
    );
    assert!(ports.customers().get(b).await.unwrap().is_some());
}
