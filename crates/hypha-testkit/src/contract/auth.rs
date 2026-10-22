use hypha_auth::{
    ApiKey, ApiKeyKind, ApiKeyParts, ApiKeyRepo, AuthSettings, CredentialKind, Identity,
    IdentityRepo, PasswordHash, PasswordReset, PasswordResetRepo, Principal, PrincipalKind, Sealed,
    Session, SessionClient, SessionRepo, TokenHash, keys,
};
use hypha_core::{
    Actor, ApiKeyId, ChannelId, Email, IdentityId, PasswordResetId, RepoError, Scope, SessionId,
    Timestamp, UserId, Version, Versioned,
};
use uuid::Uuid;

use super::scenario;

// ---------- fixtures ----------

pub(super) fn identity_id(n: u128) -> IdentityId {
    IdentityId::from_uuid(Uuid::from_u128(n))
}

fn staff(n: u128) -> Principal {
    Principal::Staff(UserId::from_uuid(Uuid::from_u128(n)))
}

fn hash(n: u8) -> TokenHash {
    TokenHash::from_bytes([n; 32])
}

pub(super) fn identity(n: u128, email: &str) -> Identity {
    Identity::with_password(
        identity_id(n),
        staff(n),
        Email::new(email).unwrap(),
        PasswordHash::from_phc(format!("$argon2id$v=19$m=8,t=1,p=1$c2FsdA$h{n}")),
    )
}

/// Every credential kind: password, a confirmed authenticator, recovery codes.
fn identity_with_mfa(n: u128, email: &str) -> Identity {
    let mut i = identity(n, email);
    i.start_totp(Sealed::from_bytes(vec![1, 0, 255, 7]))
        .unwrap();
    i.confirm_totp(1_234_567, vec![hash(1), hash(2)]).unwrap();
    i
}

fn settings() -> AuthSettings {
    AuthSettings::default()
}

pub(super) fn session(n: u128, identity: u128, token: u8) -> Session {
    Session::active(
        SessionId::from_uuid(Uuid::from_u128(n)),
        identity_id(identity),
        staff(identity),
        hash(token),
        Timestamp::from_unix_micros(1_700_000_000_000_001),
        None,
        settings().session_lifetime,
    )
}

fn key(n: u128, token: u8, kind: ApiKeyKind) -> ApiKey {
    let (scopes, channel) = match kind {
        ApiKeyKind::Secret => (Some(vec![Scope::OrdersRead, Scope::ProductsWrite]), None),
        ApiKeyKind::Publishable => (None, Some(ChannelId::from_uuid(Uuid::from_u128(500)))),
    };
    let parts = ApiKeyParts {
        id: ApiKeyId::from_uuid(Uuid::from_u128(n)),
        kind,
        name: format!("Key {n}"),
        scopes,
        channel,
        created_by: Actor::User(UserId::from_uuid(Uuid::from_u128(77))),
        created_at: Timestamp::from_unix_micros(1_700_000_000_000_001),
    };
    ApiKey::new(parts, hash(token), "a1b2").unwrap()
}

pub(super) fn reset(n: u128, identity: u128, token: u8) -> PasswordReset {
    PasswordReset::new(
        PasswordResetId::from_uuid(Uuid::from_u128(n)),
        identity_id(identity),
        hash(token),
        Timestamp::from_unix_micros(1_700_000_000_000_001),
        settings().password_reset_lifetime,
    )
}

// ---------- identities ----------

/// Runs every `IdentityRepo` scenario, each on a fresh repo from `make`.
pub async fn identity_repo<R: IdentityRepo>(make: impl AsyncFn() -> R) {
    scenario("identity: insert stores every credential at version 1");
    identity_round_trips(&make().await).await;
    scenario("identity: update replaces the credentials and bumps the version");
    identity_update(&make().await).await;
    scenario("identity: unique ID, principal and login");
    identity_keys(&make().await).await;
    scenario("identity: stale and missing updates fail");
    identity_stale(&make().await).await;
    scenario("identity: a customer signing in with an email code round trips");
    customer_identity(&make().await).await;
    scenario("identity: an agent's key round trips and is found by its hash");
    agent_identity(&make().await).await;
}

async fn agent_identity<R: IdentityRepo>(repo: &R) {
    use hypha_auth::{NewToken, TokenKind, agent_key_lookup};
    let agent = hypha_core::AgentId::from_uuid(Uuid::from_u128(41));
    let key = NewToken::generate(TokenKind::AgentKey, &crate::TestEnv::default());
    let mut i = Identity::with_agent_key(identity_id(5), agent, &key);
    repo.insert(&mut i).await.unwrap();
    assert_eq!(
        repo.get_by_principal(Principal::Agent(agent)).await,
        Ok(Some(i.clone()))
    );
    assert_eq!(
        repo.find_login(
            PrincipalKind::Agent,
            CredentialKind::AgentKey,
            &agent_key_lookup(key.hash())
        )
        .await,
        Ok(Some(i))
    );
}

async fn customer_identity<R: IdentityRepo>(repo: &R) {
    let customer = hypha_core::CustomerId::from_uuid(Uuid::from_u128(40));
    let mut i = Identity::with_email_code(
        identity_id(4),
        Principal::Customer(customer),
        Email::new("nina@example.com").unwrap(),
    );
    repo.insert(&mut i).await.unwrap();
    assert_eq!(
        repo.get_by_principal(Principal::Customer(customer)).await,
        Ok(Some(i.clone()))
    );
    assert_eq!(
        repo.find_login(
            PrincipalKind::Customer,
            CredentialKind::EmailCode,
            "nina@example.com"
        )
        .await,
        Ok(Some(i.clone()))
    );
    // A staff member may share the email: logins are unique per principal kind.
    assert_eq!(
        repo.find_login(
            PrincipalKind::Staff,
            CredentialKind::Password,
            "nina@example.com"
        )
        .await,
        Ok(None)
    );
    i.set_email_code(hash(8), Timestamp::from_unix_micros(600_000_000))
        .unwrap();
    i.use_email_code(hash(1), Timestamp::from_unix_micros(1_000_000));
    repo.update(&mut i).await.unwrap();
    assert_eq!(repo.get(i.id()).await, Ok(Some(i.clone())));
    let mut twin = Identity::with_email_code(
        identity_id(5),
        Principal::Customer(hypha_core::CustomerId::from_uuid(Uuid::from_u128(41))),
        Email::new("nina@example.com").unwrap(),
    );
    assert_eq!(
        repo.insert(&mut twin).await,
        Err(RepoError::Duplicate(keys::IDENTITY_LOGIN))
    );
}

async fn identity_round_trips<R: IdentityRepo>(repo: &R) {
    let mut i = identity_with_mfa(1, "ana@example.com");
    repo.insert(&mut i).await.unwrap();
    assert_eq!(i.version(), Version::new(1));
    assert_eq!(repo.get(i.id()).await, Ok(Some(i.clone())));
    assert_eq!(repo.get_by_principal(staff(1)).await, Ok(Some(i.clone())));
    assert_eq!(repo.find_principal(staff(1)).await, Ok(Some(i.id())));
    assert_eq!(repo.find_principal(staff(9)).await, Ok(None));
    assert_eq!(
        repo.find_login(
            PrincipalKind::Staff,
            CredentialKind::Password,
            "ana@example.com"
        )
        .await,
        Ok(Some(i))
    );
    assert_eq!(
        repo.find_login(
            PrincipalKind::Staff,
            CredentialKind::Password,
            "bob@example.com"
        )
        .await,
        Ok(None)
    );
    assert_eq!(repo.get(identity_id(9)).await, Ok(None));
    assert_eq!(repo.get_by_principal(staff(9)).await, Ok(None));
}

async fn identity_update<R: IdentityRepo>(repo: &R) {
    let mut i = identity(1, "ana@example.com");
    repo.insert(&mut i).await.unwrap();
    i.start_totp(Sealed::from_bytes(vec![9, 9])).unwrap();
    i.set_password_hash(PasswordHash::from_phc("$argon2id$new".to_string()))
        .unwrap();
    repo.update(&mut i).await.unwrap();
    assert_eq!(i.version(), Version::new(2));
    assert_eq!(repo.get(i.id()).await, Ok(Some(i.clone())));

    // Credentials removed by an update are gone.
    i.start_totp(Sealed::from_bytes(vec![1])).unwrap();
    i.confirm_totp(5, vec![hash(3)]).unwrap();
    repo.update(&mut i).await.unwrap();
    i.disable_mfa().unwrap();
    repo.update(&mut i).await.unwrap();
    assert_eq!(repo.get(i.id()).await, Ok(Some(i.clone())));
    assert_eq!(i.credentials().len(), 1);
}

async fn identity_keys<R: IdentityRepo>(repo: &R) {
    let mut ana = identity(1, "ana@example.com");
    repo.insert(&mut ana).await.unwrap();

    let mut same_id = identity(1, "other@example.com");
    assert_eq!(
        repo.insert(&mut same_id).await,
        Err(RepoError::Duplicate(keys::IDENTITY_ID))
    );

    let mut same_principal = Identity::with_password(
        identity_id(2),
        staff(1),
        Email::new("other@example.com").unwrap(),
        PasswordHash::from_phc("x".to_string()),
    );
    assert_eq!(
        repo.insert(&mut same_principal).await,
        Err(RepoError::Duplicate(keys::IDENTITY_PRINCIPAL))
    );

    let mut same_login = identity(3, "ana@example.com");
    assert_eq!(
        repo.insert(&mut same_login).await,
        Err(RepoError::Duplicate(keys::IDENTITY_LOGIN))
    );

    // Second factors have no lookup, so two identities can both have them.
    let mut a = identity_with_mfa(4, "a@example.com");
    let mut b = identity_with_mfa(5, "b@example.com");
    repo.insert(&mut a).await.unwrap();
    repo.insert(&mut b).await.unwrap();
}

async fn identity_stale<R: IdentityRepo>(repo: &R) {
    let mut i = identity(1, "ana@example.com");
    repo.insert(&mut i).await.unwrap();
    let mut old = i.clone();
    i.start_totp(Sealed::from_bytes(vec![1])).unwrap();
    repo.update(&mut i).await.unwrap();
    old.start_totp(Sealed::from_bytes(vec![2])).unwrap();
    assert_eq!(repo.update(&mut old).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(i.id()).await, Ok(Some(i)));

    let mut missing = identity(9, "nobody@example.com");
    missing.set_version(Version::new(1));
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

// ---------- sessions ----------

/// Runs every `SessionRepo` scenario, each on a fresh repo from `make`.
pub async fn session_repo<R: SessionRepo>(make: impl AsyncFn() -> R) {
    scenario("session: insert stores every field at version 1, by ID and by token");
    session_round_trips(&make().await).await;
    scenario("session: update stores every field and bumps the version");
    session_update(&make().await).await;
    scenario("session: unique ID and token");
    session_keys(&make().await).await;
    scenario("session: stale and missing updates fail");
    session_stale(&make().await).await;
    scenario("session: prune deletes sessions that ended before a time");
    session_prune(&make().await).await;
    scenario("session: the client and last use round trip");
    session_client(&make().await).await;
    scenario("session: live_for_identity is the identity's unended sessions, newest first");
    session_live_for_identity(&make().await).await;
    scenario("session: record_use moves last_used_at forward only, keeping the version");
    session_record_use(&make().await).await;
}

async fn session_client<R: SessionRepo>(repo: &R) {
    let mut s = session(1, 1, 1);
    s.set_client(SessionClient::new(
        Some("2001:db8::1"),
        Some("Mozilla/5.0 (Macintosh) Firefox/140.0"),
    ));
    repo.insert(&mut s).await.unwrap();
    assert_eq!(repo.get(s.id()).await, Ok(Some(s.clone())));
    let mut bare = session(2, 1, 2);
    repo.insert(&mut bare).await.unwrap();
    assert_eq!(
        repo.get(bare.id())
            .await
            .map(|s| s.map(|s| s.client().clone())),
        Ok(Some(SessionClient::default()))
    );
}

async fn session_live_for_identity<R: SessionRepo>(repo: &R) {
    let created = Timestamp::from_unix_micros(1_700_000_000_000_001);
    let now = created.plus_secs(60);
    let mut older = session(1, 1, 1);
    let mut newer = Session::active(
        SessionId::from_uuid(Uuid::from_u128(2)),
        identity_id(1),
        staff(1),
        hash(2),
        created.plus_secs(1),
        None,
        settings().session_lifetime,
    );
    let mut revoked = session(3, 1, 3);
    revoked.revoke(created);
    let mut expired = Session::pending(
        SessionId::from_uuid(Uuid::from_u128(4)),
        identity_id(1),
        staff(1),
        hash(4),
        created,
        core::time::Duration::from_secs(30),
    );
    let mut pending = Session::pending(
        SessionId::from_uuid(Uuid::from_u128(5)),
        identity_id(1),
        staff(1),
        hash(5),
        now,
        settings().second_factor_window,
    );
    let mut other = session(6, 2, 6);
    for s in [
        &mut older,
        &mut newer,
        &mut revoked,
        &mut expired,
        &mut pending,
        &mut other,
    ] {
        repo.insert(s).await.unwrap();
    }
    assert_eq!(
        repo.live_for_identity(identity_id(1), now).await,
        Ok(vec![pending, newer, older])
    );
    assert_eq!(
        repo.live_for_identity(identity_id(9), now).await,
        Ok(vec![])
    );
}

async fn session_record_use<R: SessionRepo>(repo: &R) {
    let mut s = session(1, 1, 1);
    repo.insert(&mut s).await.unwrap();
    let later = s.created_at().plus_secs(3600);
    repo.record_use(s.id(), later).await.unwrap();
    let stored = repo.get(s.id()).await.unwrap().unwrap();
    assert_eq!(stored.last_used_at(), later);
    assert_eq!(stored.version(), Version::new(1));
    // Never back.
    repo.record_use(s.id(), s.created_at()).await.unwrap();
    assert_eq!(
        repo.get(s.id()).await.map(|s| s.map(|s| s.last_used_at())),
        Ok(Some(later))
    );
    // An update from a copy loaded before the use keeps it, and the copy learns it.
    s.revoke(later);
    repo.update(&mut s).await.unwrap();
    assert_eq!(s.last_used_at(), later);
    assert_eq!(repo.get(s.id()).await, Ok(Some(s)));
    // A session that's gone is no error.
    let missing = SessionId::from_uuid(Uuid::from_u128(99));
    assert_eq!(repo.record_use(missing, later).await, Ok(()));
}

async fn session_prune<R: SessionRepo>(repo: &R) {
    let created = Timestamp::from_unix_micros(1_700_000_000_000_001);
    let mut revoked = session(1, 1, 1);
    revoked.revoke(created.plus_secs(10));
    for mut s in [revoked, session(2, 1, 2), session(3, 2, 3)] {
        repo.insert(&mut s).await.unwrap();
    }
    // Only the revoked one had ended by then.
    assert_eq!(repo.prune(created.plus_secs(20), 10).await, Ok(1));
    assert_eq!(
        repo.get(SessionId::from_uuid(Uuid::from_u128(1))).await,
        Ok(None)
    );
    // Later, the others have expired too.
    let later = created.plus(settings().session_lifetime).plus_secs(1);
    assert_eq!(repo.prune(later, 10).await, Ok(2));
    assert_eq!(repo.prune(later, 10).await, Ok(0));
}

async fn session_round_trips<R: SessionRepo>(repo: &R) {
    let mut active = session(1, 1, 1);
    repo.insert(&mut active).await.unwrap();
    assert_eq!(active.version(), Version::new(1));
    assert_eq!(repo.get(active.id()).await, Ok(Some(active.clone())));
    assert_eq!(repo.get_by_token(hash(1)).await, Ok(Some(active)));

    let mut pending = Session::pending(
        SessionId::from_uuid(Uuid::from_u128(2)),
        identity_id(1),
        staff(1),
        hash(2),
        Timestamp::from_unix_micros(-5),
        settings().second_factor_window,
    );
    pending.record_failed_attempt(Timestamp::UNIX_EPOCH);
    repo.insert(&mut pending).await.unwrap();
    assert_eq!(repo.get(pending.id()).await, Ok(Some(pending)));
    assert_eq!(repo.get_by_token(hash(9)).await, Ok(None));
}

async fn session_update<R: SessionRepo>(repo: &R) {
    let mut s = Session::pending(
        SessionId::from_uuid(Uuid::from_u128(1)),
        identity_id(1),
        staff(1),
        hash(1),
        Timestamp::UNIX_EPOCH,
        settings().second_factor_window,
    );
    repo.insert(&mut s).await.unwrap();
    s.activate(
        hash(2),
        Timestamp::from_unix_micros(7),
        settings().session_lifetime,
    );
    s.revoke(Timestamp::from_unix_micros(8));
    repo.update(&mut s).await.unwrap();
    assert_eq!(s.version(), Version::new(2));
    assert_eq!(repo.get(s.id()).await, Ok(Some(s.clone())));
    assert_eq!(repo.get_by_token(hash(2)).await, Ok(Some(s)));
    assert_eq!(repo.get_by_token(hash(1)).await, Ok(None));
}

async fn session_keys<R: SessionRepo>(repo: &R) {
    let mut a = session(1, 1, 1);
    repo.insert(&mut a).await.unwrap();
    let mut same_id = session(1, 2, 2);
    assert_eq!(
        repo.insert(&mut same_id).await,
        Err(RepoError::Duplicate(keys::SESSION_ID))
    );
    let mut same_token = session(2, 2, 1);
    assert_eq!(
        repo.insert(&mut same_token).await,
        Err(RepoError::Duplicate(keys::SESSION_TOKEN))
    );
    // Activation can't take another session's token either.
    let mut b = Session::pending(
        SessionId::from_uuid(Uuid::from_u128(3)),
        identity_id(1),
        staff(1),
        hash(3),
        Timestamp::UNIX_EPOCH,
        settings().second_factor_window,
    );
    repo.insert(&mut b).await.unwrap();
    b.activate(hash(1), Timestamp::UNIX_EPOCH, settings().session_lifetime);
    assert_eq!(
        repo.update(&mut b).await,
        Err(RepoError::Duplicate(keys::SESSION_TOKEN))
    );
}

async fn session_stale<R: SessionRepo>(repo: &R) {
    let mut s = session(1, 1, 1);
    repo.insert(&mut s).await.unwrap();
    let mut old = s.clone();
    s.revoke(Timestamp::UNIX_EPOCH);
    repo.update(&mut s).await.unwrap();
    assert_eq!(repo.update(&mut old).await, Err(RepoError::StaleVersion));
    let mut missing = session(9, 1, 9);
    missing.set_version(Version::new(1));
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

// ---------- API keys ----------

/// Runs every `ApiKeyRepo` scenario, each on a fresh repo from `make`.
pub async fn api_key_repo<R: ApiKeyRepo>(make: impl AsyncFn() -> R) {
    scenario("api key: insert stores every field at version 1, by ID and by token");
    key_round_trips(&make().await).await;
    scenario("api key: update stores the revocation and bumps the version");
    key_update(&make().await).await;
    scenario("api key: unique ID and token");
    key_keys(&make().await).await;
    scenario("api key: keys come in pages, newest first");
    key_pages(&make().await).await;
}

async fn key_pages<R: ApiKeyRepo>(repo: &R) {
    let first = |limit| hypha_core::PageRequest {
        after: None,
        limit: hypha_core::PageLimit::new(limit).unwrap(),
    };
    assert_eq!(repo.list(first(10)).await.unwrap().items, vec![]);
    for n in [1, 3, 2] {
        repo.insert(&mut key(n, n as u8, ApiKeyKind::Secret))
            .await
            .unwrap();
    }
    let page = repo.list(first(2)).await.unwrap();
    let ids: Vec<ApiKeyId> = page.items.iter().map(|k| k.id()).collect();
    assert_eq!(
        (ids, page.has_more),
        (
            vec![
                ApiKeyId::from_uuid(Uuid::from_u128(3)),
                ApiKeyId::from_uuid(Uuid::from_u128(2))
            ],
            true
        )
    );
    let rest = repo
        .list(hypha_core::PageRequest {
            after: Some(ApiKeyId::from_uuid(Uuid::from_u128(2))),
            limit: hypha_core::PageLimit::new(2).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(
        rest.items,
        vec![
            repo.get(ApiKeyId::from_uuid(Uuid::from_u128(1)))
                .await
                .unwrap()
                .unwrap()
        ]
    );
    assert!(!rest.has_more);
}

async fn key_round_trips<R: ApiKeyRepo>(repo: &R) {
    for (n, kind) in [(1, ApiKeyKind::Secret), (2, ApiKeyKind::Publishable)] {
        let mut k = key(n, n as u8, kind);
        repo.insert(&mut k).await.unwrap();
        assert_eq!(k.version(), Version::new(1));
        assert_eq!(repo.get(k.id()).await, Ok(Some(k.clone())));
        assert_eq!(repo.get_by_token(hash(n as u8)).await, Ok(Some(k)));
    }
    // Each actor kind survives.
    for (n, actor) in [
        (3, Actor::System),
        (4, Actor::ApiKey(ApiKeyId::from_uuid(Uuid::from_u128(1)))),
        (
            5,
            Actor::Agent(hypha_core::AgentId::from_uuid(Uuid::from_u128(1))),
        ),
    ] {
        let parts = ApiKeyParts {
            id: ApiKeyId::from_uuid(Uuid::from_u128(n)),
            kind: ApiKeyKind::Secret,
            name: "Key".to_string(),
            scopes: Some(vec![Scope::Checkout]),
            channel: None,
            created_by: actor,
            created_at: Timestamp::UNIX_EPOCH,
        };
        let mut k = ApiKey::new(parts, hash(n as u8), "zzzz").unwrap();
        repo.insert(&mut k).await.unwrap();
        assert_eq!(repo.get(k.id()).await, Ok(Some(k)));
    }
    assert_eq!(repo.get_by_token(hash(99)).await, Ok(None));
}

async fn key_update<R: ApiKeyRepo>(repo: &R) {
    let mut k = key(1, 1, ApiKeyKind::Secret);
    repo.insert(&mut k).await.unwrap();
    let mut old = k.clone();
    k.revoke(Timestamp::from_unix_micros(42));
    repo.update(&mut k).await.unwrap();
    assert_eq!(k.version(), Version::new(2));
    assert_eq!(repo.get(k.id()).await, Ok(Some(k)));
    old.revoke(Timestamp::UNIX_EPOCH);
    assert_eq!(repo.update(&mut old).await, Err(RepoError::StaleVersion));
}

async fn key_keys<R: ApiKeyRepo>(repo: &R) {
    let mut k = key(1, 1, ApiKeyKind::Secret);
    repo.insert(&mut k).await.unwrap();
    assert_eq!(
        repo.insert(&mut key(1, 2, ApiKeyKind::Secret)).await,
        Err(RepoError::Duplicate(keys::API_KEY_ID))
    );
    assert_eq!(
        repo.insert(&mut key(2, 1, ApiKeyKind::Secret)).await,
        Err(RepoError::Duplicate(keys::API_KEY_TOKEN))
    );
}

// ---------- password resets ----------

/// Runs every `PasswordResetRepo` scenario, each on a fresh repo from `make`.
pub async fn password_reset_repo<R: PasswordResetRepo>(make: impl AsyncFn() -> R) {
    scenario("password reset: insert stores every field at version 1, by token");
    reset_round_trips(&make().await).await;
    scenario("password reset: unique ID and token");
    reset_keys(&make().await).await;
    scenario("password reset: the latest one per identity");
    reset_latest(&make().await).await;
}

async fn reset_latest<R: PasswordResetRepo>(repo: &R) {
    assert_eq!(repo.latest_for_identity(identity_id(1)).await, Ok(None));
    let at = |micros, n, token| {
        PasswordReset::new(
            PasswordResetId::from_uuid(Uuid::from_u128(n)),
            identity_id(1),
            hash(token),
            Timestamp::from_unix_micros(micros),
            settings().password_reset_lifetime,
        )
    };
    let mut older = at(1_000, 1, 1);
    let mut newer = at(2_000, 2, 2);
    let mut other = reset(3, 2, 3);
    // Inserted newest first, so insertion order can't be what decides.
    repo.insert(&mut newer).await.unwrap();
    repo.insert(&mut older).await.unwrap();
    repo.insert(&mut other).await.unwrap();
    assert_eq!(
        repo.latest_for_identity(identity_id(1)).await,
        Ok(Some(newer))
    );
    assert_eq!(
        repo.latest_for_identity(identity_id(2)).await,
        Ok(Some(other))
    );
}

async fn reset_round_trips<R: PasswordResetRepo>(repo: &R) {
    let mut r = reset(1, 1, 1);
    repo.insert(&mut r).await.unwrap();
    assert_eq!(r.version(), Version::new(1));
    assert_eq!(repo.get_by_token(hash(1)).await, Ok(Some(r)));
    assert_eq!(repo.get_by_token(hash(2)).await, Ok(None));
}

async fn reset_keys<R: PasswordResetRepo>(repo: &R) {
    repo.insert(&mut reset(1, 1, 1)).await.unwrap();
    assert_eq!(
        repo.insert(&mut reset(1, 2, 2)).await,
        Err(RepoError::Duplicate(keys::PASSWORD_RESET_ID))
    );
    assert_eq!(
        repo.insert(&mut reset(2, 2, 1)).await,
        Err(RepoError::Duplicate(keys::PASSWORD_RESET_TOKEN))
    );
}
