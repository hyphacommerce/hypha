use hypha_core::{
    Actor, AgentId, ApiKeyId, HasPreviewTokens, PreviewToken, PreviewTokenRepo, RepoError,
    Timestamp, UserId, preview_token::keys,
};
use uuid::Uuid;

use super::scenario;

/// Runs every `PreviewTokenRepo` scenario, each on fresh ports from `make`.
pub async fn preview_token_repo<S: HasPreviewTokens>(make: impl AsyncFn() -> S) {
    scenario("preview tokens: a token round-trips for every kind of actor");
    round_trips(&make().await).await;
    scenario("preview tokens: a hash is stored once");
    duplicate(&make().await).await;
    scenario("preview tokens: a token is marked used once; a missing one never");
    used_once(&make().await).await;
    scenario("preview tokens: prune deletes tokens expired before a time, up to a limit");
    prunes(&make().await).await;
}

fn token(n: u8, actor: Actor, expires_at: i64) -> PreviewToken {
    PreviewToken {
        hash: [n; 32],
        actor,
        request: [n.wrapping_add(1); 32],
        ids: vec![Uuid::from_u128(u128::from(n)), Uuid::from_u128(7)],
        digest: format!("sha256:{n:02x}"),
        expires_at: Timestamp::from_unix_micros(expires_at),
        used_at: None,
    }
}

fn user() -> Actor {
    Actor::User(UserId::from_uuid(Uuid::from_u128(1)))
}

async fn round_trips<S: HasPreviewTokens>(ports: &S) {
    let repo = ports.preview_tokens();
    let actors = [
        user(),
        Actor::Agent(AgentId::from_uuid(Uuid::from_u128(2))),
        Actor::ApiKey(ApiKeyId::from_uuid(Uuid::from_u128(3))),
    ];
    for (n, actor) in actors.into_iter().enumerate() {
        let t = token(n as u8 + 1, actor, 1_000_000);
        repo.insert(&t).await.unwrap();
        assert_eq!(repo.get(&t.hash).await, Ok(Some(t)));
    }
    let no_ids = PreviewToken {
        ids: Vec::new(),
        ..token(9, user(), 5)
    };
    repo.insert(&no_ids).await.unwrap();
    assert_eq!(repo.get(&no_ids.hash).await, Ok(Some(no_ids)));
    assert_eq!(repo.get(&[0; 32]).await, Ok(None));
}

async fn duplicate<S: HasPreviewTokens>(ports: &S) {
    let repo = ports.preview_tokens();
    let first = token(1, user(), 10);
    repo.insert(&first).await.unwrap();
    let again = PreviewToken {
        digest: "sha256:ff".to_string(),
        ..first.clone()
    };
    assert_eq!(
        repo.insert(&again).await,
        Err(RepoError::Duplicate(keys::PREVIEW_TOKEN))
    );
    assert_eq!(repo.get(&first.hash).await, Ok(Some(first)));
}

async fn used_once<S: HasPreviewTokens>(ports: &S) {
    let repo = ports.preview_tokens();
    let t = token(1, user(), 10);
    repo.insert(&t).await.unwrap();
    let at = Timestamp::from_unix_micros(3);
    assert_eq!(repo.mark_used(&t.hash, at).await, Ok(true));
    assert_eq!(
        repo.mark_used(&t.hash, Timestamp::from_unix_micros(4))
            .await,
        Ok(false)
    );
    let stored = repo.get(&t.hash).await.unwrap().unwrap();
    assert_eq!(stored.used_at, Some(at));
    assert_eq!(repo.mark_used(&[0; 32], at).await, Ok(false));
}

async fn prunes<S: HasPreviewTokens>(ports: &S) {
    let repo = ports.preview_tokens();
    for (n, at) in [(1, 1), (2, 2), (3, 3), (4, 10)] {
        repo.insert(&token(n, user(), at)).await.unwrap();
    }
    let cutoff = Timestamp::from_unix_micros(10);
    assert_eq!(repo.prune(cutoff, 2).await, Ok(2));
    assert_eq!(repo.prune(cutoff, 10).await, Ok(1));
    assert_eq!(repo.prune(cutoff, 10).await, Ok(0));
    assert!(repo.get(&[4; 32]).await.unwrap().is_some());
}
