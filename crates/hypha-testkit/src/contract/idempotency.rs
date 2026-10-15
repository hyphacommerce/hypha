use hypha_core::{
    Actor, AgentId, HasBatch, HasIdempotency, IdempotencyKey, IdempotencyRecord, IdempotencyRepo,
    RepoError, StageIdempotency, Timestamp, UserId, idempotency::keys,
};
use uuid::Uuid;

use super::scenario;

/// Runs every `IdempotencyRepo` scenario, each on fresh ports from `make`. Records are
/// only written through a batch, so this needs `HasBatch` too.
pub async fn idempotency_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasBatch + HasIdempotency,
    S::Batch: StageIdempotency,
{
    scenario("idempotency: a record round-trips for every kind of actor");
    round_trips(&make().await).await;
    scenario("idempotency: keys are per actor; an unknown key is None");
    keys_are_per_actor(&make().await).await;
    scenario("idempotency: a key the actor already used fails, keeping the first");
    duplicate_key(&make().await).await;
    scenario("idempotency: prune deletes records created before a time, up to a limit");
    prunes(&make().await).await;
}

async fn prunes<S>(ports: &S)
where
    S: HasBatch + HasIdempotency,
    S::Batch: StageIdempotency,
{
    let at = |micros: i64| {
        let mut r = record(user(), &format!("k{micros}"), micros as u128);
        r.created_at = Timestamp::from_unix_micros(micros);
        r
    };
    write(ports, vec![at(1), at(2), at(3), at(10)])
        .await
        .unwrap();
    let repo = ports.idempotency();
    let cutoff = Timestamp::from_unix_micros(10);
    assert_eq!(repo.prune(cutoff, 2).await, Ok(2));
    assert_eq!(repo.prune(cutoff, 10).await, Ok(1));
    assert_eq!(repo.prune(cutoff, 10).await, Ok(0));
    assert!(repo.get(user(), &key("k10")).await.unwrap().is_some());
    assert_eq!(repo.get(user(), &key("k1")).await, Ok(None));
}

fn user() -> Actor {
    Actor::User(UserId::from_uuid(Uuid::from_u128(1)))
}

fn key(s: &str) -> IdempotencyKey {
    IdempotencyKey::new(s).unwrap()
}

fn record(actor: Actor, k: &str, result: u128) -> IdempotencyRecord {
    IdempotencyRecord {
        actor,
        key: key(k),
        operation: "create_cart".to_string(),
        // Every byte different, so a reordered or truncated fingerprint shows.
        fingerprint: std::array::from_fn(|i| (result as u8).wrapping_add(i as u8)),
        result: Uuid::from_u128(result),
        // A microsecond after 2026-01-01, so precision is checked too.
        created_at: Timestamp::from_unix_micros(1_767_225_600_000_001),
    }
}

async fn write<S>(ports: &S, records: Vec<IdempotencyRecord>) -> Result<(), RepoError>
where
    S: HasBatch,
    S::Batch: StageIdempotency,
{
    let mut batch = ports.batch();
    for r in records {
        batch.record_idempotency(r);
    }
    ports.commit(batch).await
}

async fn round_trips<S>(ports: &S)
where
    S: HasBatch + HasIdempotency,
    S::Batch: StageIdempotency,
{
    let agent = Actor::Agent(AgentId::from_uuid(Uuid::from_u128(2)));
    let records = vec![
        record(Actor::System, "k-1", 1),
        record(user(), "k-1", 2),
        record(agent, &"k".repeat(255), 3),
    ];
    write(ports, records.clone()).await.unwrap();
    for r in records {
        assert_eq!(
            ports.idempotency().get(r.actor, &r.key).await,
            Ok(Some(r.clone()))
        );
    }
}

async fn keys_are_per_actor<S>(ports: &S)
where
    S: HasBatch + HasIdempotency,
    S::Batch: StageIdempotency,
{
    write(ports, vec![record(user(), "k-1", 1)]).await.unwrap();
    assert_eq!(
        ports.idempotency().get(Actor::System, &key("k-1")).await,
        Ok(None)
    );
    assert_eq!(ports.idempotency().get(user(), &key("k-2")).await, Ok(None));
}

async fn duplicate_key<S>(ports: &S)
where
    S: HasBatch + HasIdempotency,
    S::Batch: StageIdempotency,
{
    let first = record(user(), "k-1", 1);
    write(ports, vec![first.clone()]).await.unwrap();
    assert_eq!(
        write(
            ports,
            vec![record(user(), "k-2", 3), record(user(), "k-1", 2)]
        )
        .await,
        Err(RepoError::Duplicate(keys::IDEMPOTENCY_KEY))
    );
    assert_eq!(
        ports.idempotency().get(user(), &key("k-1")).await,
        Ok(Some(first))
    );
    // The batch failed as a whole: its other record isn't there either.
    assert_eq!(ports.idempotency().get(user(), &key("k-2")).await, Ok(None));
}
