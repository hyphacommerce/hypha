use hypha_core::{
    Actor, AuthError, Ctx, FulfillmentId, LocationId, OrderId, OrderLineId, Quantity, Scope,
    Timestamp, UserId, Version, Versioned,
};
use hypha_fulfillment::{
    CommandError, Fulfillment, FulfillmentError, FulfillmentLine, FulfillmentRepo, NewFulfillment,
    Tracking, TrackingInput, get_fulfillment, list_order_fulfillments, update_tracking,
};
use hypha_testkit::InMemoryFulfillmentRepo;
use pollster::block_on;
use uuid::Uuid;

fn fid(n: u128) -> FulfillmentId {
    FulfillmentId::from_uuid(Uuid::from_u128(n))
}

fn order() -> OrderId {
    OrderId::from_uuid(Uuid::from_u128(100))
}

/// A repo holding fulfilments 1 and 2 of `order()`, each with tracking number "1".
fn repo() -> InMemoryFulfillmentRepo {
    let repo = InMemoryFulfillmentRepo::default();
    for n in [1, 2] {
        let mut f = Fulfillment::new(NewFulfillment {
            id: fid(n),
            order: order(),
            location: LocationId::from_uuid(Uuid::from_u128(200)),
            lines: vec![FulfillmentLine {
                line: OrderLineId::from_uuid(Uuid::from_u128(300 + n)),
                quantity: Quantity::new(1).unwrap(),
            }],
            tracking: vec![Tracking::new(None, "1", None).unwrap()],
        })
        .unwrap();
        block_on(repo.insert(&mut f)).unwrap();
    }
    repo
}

fn user_with(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(9))), scopes)
}

fn input(number: &str, url: Option<&str>) -> TrackingInput {
    TrackingInput {
        carrier: Some("GLS".to_string()),
        number: number.to_string(),
        url: url.map(str::to_string),
    }
}

#[test]
fn get_fulfillment_returns_it() {
    let repo = repo();
    let f = block_on(get_fulfillment(&Ctx::system(), &repo, fid(1))).unwrap();
    assert_eq!(f.order(), order());
}

#[test]
fn get_fulfillment_reports_an_unknown_one() {
    assert_eq!(
        block_on(get_fulfillment(&Ctx::system(), &repo(), fid(9))),
        Err(CommandError::FulfillmentNotFound(fid(9)))
    );
}

#[test]
fn reading_needs_fulfillments_read() {
    let repo = repo();
    // `orders:write` doesn't grant it: shipping is its own permission (FU9).
    let ctx = user_with(vec![Scope::OrdersWrite]);
    let missing = CommandError::Auth(AuthError::MissingScope(Scope::FulfillmentsRead));
    assert_eq!(
        block_on(get_fulfillment(&ctx, &repo, fid(1))),
        Err(missing.clone())
    );
    assert_eq!(
        block_on(list_order_fulfillments(&ctx, &repo, order())),
        Err(missing)
    );
    let writer = user_with(vec![Scope::FulfillmentsWrite]);
    assert!(block_on(get_fulfillment(&writer, &repo, fid(1))).is_ok());
}

#[test]
fn an_orders_fulfilments_come_newest_first() {
    let listed = block_on(list_order_fulfillments(&Ctx::system(), &repo(), order())).unwrap();
    let ids: Vec<FulfillmentId> = listed.iter().map(Fulfillment::id).collect();
    assert_eq!(ids, [fid(2), fid(1)]);
}

#[test]
fn updating_tracking_stores_it() {
    let repo = repo();
    let (f, changed) = block_on(update_tracking(
        &Ctx::system(),
        &repo,
        fid(1),
        &[input("2", Some("https://gls.example/2"))],
    ))
    .unwrap();
    assert!(changed);
    assert_eq!(f.version(), Version::new(2));
    assert_eq!(block_on(repo.get(fid(1))), Ok(Some(f)));
    assert_eq!(
        repo.outbox().types(),
        [
            "fulfillment.created",
            "fulfillment.created",
            "fulfillment.updated"
        ]
    );
}

#[test]
fn the_same_tracking_writes_nothing() {
    let repo = repo();
    let tracking = [TrackingInput {
        carrier: None,
        number: "1".to_string(),
        url: None,
    }];
    let (f, changed) = block_on(update_tracking(&Ctx::system(), &repo, fid(1), &tracking)).unwrap();
    assert!(!changed);
    assert_eq!(f.version(), Version::new(1));
}

#[test]
fn bad_tracking_is_refused_before_anything_loads() {
    assert_eq!(
        block_on(update_tracking(
            &Ctx::system(),
            &repo(),
            fid(9),
            &[input("2", Some("gls.example/2"))]
        )),
        Err(CommandError::Fulfillment(
            FulfillmentError::InvalidTrackingUrl("gls.example/2".to_string())
        ))
    );
}

#[test]
fn a_cancelled_fulfilments_tracking_cant_change() {
    let repo = repo();
    let mut f = block_on(repo.get(fid(1))).unwrap().unwrap();
    f.cancel(Timestamp::UNIX_EPOCH);
    block_on(repo.update(&mut f)).unwrap();
    assert_eq!(
        block_on(update_tracking(
            &Ctx::system(),
            &repo,
            fid(1),
            &[input("2", None)]
        )),
        Err(CommandError::Fulfillment(FulfillmentError::Cancelled(fid(
            1
        ))))
    );
}

#[test]
fn updating_tracking_needs_fulfillments_write() {
    let ctx = user_with(vec![Scope::FulfillmentsRead]);
    assert_eq!(
        block_on(update_tracking(&ctx, &repo(), fid(1), &[])),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::FulfillmentsWrite
        )))
    );
}
