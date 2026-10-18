use hypha_auth::{AuthSettings, TokenHash};
use hypha_auth::{HasSessions, SessionRepo};
use hypha_core::{
    Actor, Email, InviteId, RepoError, RoleId, Scope, Timestamp, UserId, Version, Versioned,
};
use hypha_users::{
    HasRoles, HasStaffSessions, HasUsers, Invite, InviteRepo, Role, RoleRepo, StaffSessionRecord,
    StaffSessionRepo, User, UserRepo, UserStatus, keys,
};
use uuid::Uuid;

use super::{auth::session, scenario};

pub(super) fn role_id(n: u128) -> RoleId {
    RoleId::from_uuid(Uuid::from_u128(n))
}

pub(super) fn user(n: u128, email: &str, role: u128) -> User {
    User::new(
        UserId::from_uuid(Uuid::from_u128(n)),
        Email::new(email).unwrap(),
        "Ana Horvat",
        role_id(role),
        Timestamp::from_unix_micros(1_700_000_000_000_001),
    )
    .unwrap()
}

pub(super) fn role(n: u128, name: &str) -> Role {
    Role::new(
        role_id(n),
        name,
        &[Scope::OrdersWrite, Scope::CustomersRead],
    )
    .unwrap()
}

pub(super) fn invite(n: u128, email: &str, token: u8) -> Invite {
    Invite::new(
        InviteId::from_uuid(Uuid::from_u128(n)),
        Email::new(email).unwrap(),
        role_id(1),
        TokenHash::from_bytes([token; 32]),
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        Timestamp::from_unix_micros(1_700_000_000_000_001),
        AuthSettings::default().invite_lifetime,
    )
}

// ---------- users ----------

/// Runs every `UserRepo` scenario, each on a fresh repo from `make`.
pub async fn user_repo<R: UserRepo>(make: impl AsyncFn() -> R) {
    scenario("user: insert stores every field at version 1");
    user_round_trips(&make().await).await;
    scenario("user: update stores every field and bumps the version");
    user_update(&make().await).await;
    scenario("user: unique ID and email");
    user_keys(&make().await).await;
    scenario("user: any and count_active_with_role");
    user_counts(&make().await).await;
    scenario("user: list_all is every user, oldest first");
    user_list(&make().await).await;
}

async fn user_list<R: UserRepo>(repo: &R) {
    assert_eq!(repo.list_all().await, Ok(vec![]));
    let mut b = user(2, "b@example.com", 1);
    let mut a = user(1, "a@example.com", 1);
    repo.insert(&mut b).await.unwrap();
    repo.insert(&mut a).await.unwrap();
    assert_eq!(repo.list_all().await, Ok(vec![a, b]));
}

async fn user_round_trips<R: UserRepo>(repo: &R) {
    let mut u = user(1, "ana@example.com", 1);
    repo.insert(&mut u).await.unwrap();
    assert_eq!(u.version(), Version::new(1));
    assert_eq!(repo.get(u.id()).await, Ok(Some(u.clone())));
    assert_eq!(repo.get_by_email(u.email()).await, Ok(Some(u)));
    let bob = Email::new("bob@example.com").unwrap();
    assert_eq!(repo.get_by_email(&bob).await, Ok(None));
}

async fn user_update<R: UserRepo>(repo: &R) {
    let mut u = user(1, "ana@example.com", 1);
    repo.insert(&mut u).await.unwrap();
    let mut old = u.clone();
    u.rename("Ana B.").unwrap();
    u.set_role(role_id(2));
    u.set_status(UserStatus::Disabled);
    repo.update(&mut u).await.unwrap();
    assert_eq!(u.version(), Version::new(2));
    assert_eq!(repo.get(u.id()).await, Ok(Some(u)));
    assert_eq!(repo.update(&mut old).await, Err(RepoError::StaleVersion));
}

async fn user_keys<R: UserRepo>(repo: &R) {
    repo.insert(&mut user(1, "ana@example.com", 1))
        .await
        .unwrap();
    assert_eq!(
        repo.insert(&mut user(1, "bob@example.com", 1)).await,
        Err(RepoError::Duplicate(keys::USER_ID))
    );
    assert_eq!(
        repo.insert(&mut user(2, "ana@example.com", 1)).await,
        Err(RepoError::Duplicate(keys::USER_EMAIL))
    );
}

async fn user_counts<R: UserRepo>(repo: &R) {
    assert_eq!(repo.any().await, Ok(false));
    repo.insert(&mut user(1, "a@example.com", 1)).await.unwrap();
    repo.insert(&mut user(2, "b@example.com", 1)).await.unwrap();
    repo.insert(&mut user(3, "c@example.com", 2)).await.unwrap();
    let mut disabled = user(4, "d@example.com", 1);
    disabled.set_status(UserStatus::Disabled);
    repo.insert(&mut disabled).await.unwrap();
    assert_eq!(repo.any().await, Ok(true));
    assert_eq!(repo.count_active_with_role(role_id(1)).await, Ok(2));
    assert_eq!(repo.count_active_with_role(role_id(2)).await, Ok(1));
    assert_eq!(repo.count_active_with_role(role_id(3)).await, Ok(0));
}

// ---------- roles ----------

/// Runs every `RoleRepo` scenario, each on a fresh repo from `make`.
pub async fn role_repo<R: RoleRepo>(make: impl AsyncFn() -> R) {
    scenario("role: insert stores every field at version 1");
    role_round_trips(&make().await).await;
    scenario("role: update stores every field and bumps the version");
    role_update(&make().await).await;
    scenario("role: unique ID, name ignoring case, one owner");
    role_keys(&make().await).await;
    scenario("role: list_all is every role, oldest first");
    role_list(&make().await).await;
}

async fn role_list<R: RoleRepo>(repo: &R) {
    assert_eq!(repo.list_all().await, Ok(vec![]));
    let mut support = role(2, "Support");
    let mut owner = Role::owner(role_id(1));
    repo.insert(&mut support).await.unwrap();
    repo.insert(&mut owner).await.unwrap();
    assert_eq!(repo.list_all().await, Ok(vec![owner, support]));
}

async fn role_round_trips<R: RoleRepo>(repo: &R) {
    let mut owner = Role::owner(role_id(1));
    let mut support = role(2, "Support");
    repo.insert(&mut owner).await.unwrap();
    repo.insert(&mut support).await.unwrap();
    assert_eq!(support.version(), Version::new(1));
    assert_eq!(repo.get(owner.id()).await, Ok(Some(owner.clone())));
    assert_eq!(repo.get(support.id()).await, Ok(Some(support.clone())));
    assert_eq!(repo.owner().await, Ok(Some(owner)));
    assert_eq!(repo.get_by_name("sUPPORT").await, Ok(Some(support)));
    assert_eq!(repo.get_by_name("Nope").await, Ok(None));
}

async fn role_update<R: RoleRepo>(repo: &R) {
    assert_eq!(repo.owner().await, Ok(None));
    let mut r = role(1, "Support");
    repo.insert(&mut r).await.unwrap();
    let mut old = r.clone();
    r.rename("Helpdesk").unwrap();
    r.set_scopes(&[Scope::ApiKeysRead]).unwrap();
    repo.update(&mut r).await.unwrap();
    assert_eq!(r.version(), Version::new(2));
    assert_eq!(repo.get(r.id()).await, Ok(Some(r)));
    assert_eq!(repo.update(&mut old).await, Err(RepoError::StaleVersion));
}

async fn role_keys<R: RoleRepo>(repo: &R) {
    repo.insert(&mut Role::owner(role_id(1))).await.unwrap();
    repo.insert(&mut role(2, "Support")).await.unwrap();
    assert_eq!(
        repo.insert(&mut role(2, "Other")).await,
        Err(RepoError::Duplicate(keys::ROLE_ID))
    );
    assert_eq!(
        repo.insert(&mut role(3, "SUPPORT")).await,
        Err(RepoError::Duplicate(keys::ROLE_NAME))
    );
    // Every owner role is named "Owner", so a second one breaks the name rule, the owner
    // rule or both; adapters may report either.
    let result = repo.insert(&mut Role::owner(role_id(4))).await;
    assert!(
        matches!(
            result,
            Err(RepoError::Duplicate(keys::ROLE_OWNER | keys::ROLE_NAME))
        ),
        "{result:?}"
    );
    let mut renamed = role(5, "Helpdesk");
    repo.insert(&mut renamed).await.unwrap();
    renamed.rename("support").unwrap();
    assert_eq!(
        repo.update(&mut renamed).await,
        Err(RepoError::Duplicate(keys::ROLE_NAME))
    );
}

// ---------- invites ----------

/// Runs every `InviteRepo` scenario, each on a fresh repo from `make`.
pub async fn invite_repo<R: InviteRepo>(make: impl AsyncFn() -> R) {
    scenario("invite: insert stores every field at version 1, by email and token");
    invite_round_trips(&make().await).await;
    scenario("invite: update stores every field and bumps the version");
    invite_update(&make().await).await;
    scenario("invite: unique ID, email and token");
    invite_keys(&make().await).await;
    scenario("invite: pending is every invite not accepted, oldest first");
    invite_pending(&make().await).await;
}

async fn invite_pending<R: InviteRepo>(repo: &R) {
    assert_eq!(repo.pending().await, Ok(vec![]));
    let mut later = invite(3, "c@example.com", 3);
    let mut accepted = invite(2, "b@example.com", 2);
    let mut first = invite(1, "a@example.com", 1);
    for i in [&mut later, &mut accepted, &mut first] {
        repo.insert(i).await.unwrap();
    }
    accepted.accept(Timestamp::from_unix_micros(6));
    repo.update(&mut accepted).await.unwrap();
    assert_eq!(repo.pending().await, Ok(vec![first, later]));
}

async fn invite_round_trips<R: InviteRepo>(repo: &R) {
    let mut i = invite(1, "bob@example.com", 1);
    repo.insert(&mut i).await.unwrap();
    assert_eq!(i.version(), Version::new(1));
    assert_eq!(repo.get_by_email(i.email()).await, Ok(Some(i.clone())));
    assert_eq!(
        repo.get_by_token(TokenHash::from_bytes([1; 32])).await,
        Ok(Some(i))
    );
    assert_eq!(
        repo.get_by_token(TokenHash::from_bytes([2; 32])).await,
        Ok(None)
    );
}

async fn invite_update<R: InviteRepo>(repo: &R) {
    let mut i = invite(1, "bob@example.com", 1);
    repo.insert(&mut i).await.unwrap();
    let mut old = i.clone();
    i.renew(
        TokenHash::from_bytes([2; 32]),
        role_id(2),
        Actor::System,
        Timestamp::from_unix_micros(5),
        AuthSettings::default().invite_lifetime,
    );
    i.accept(Timestamp::from_unix_micros(6));
    repo.update(&mut i).await.unwrap();
    assert_eq!(i.version(), Version::new(2));
    assert_eq!(repo.get_by_email(i.email()).await, Ok(Some(i)));
    assert_eq!(repo.update(&mut old).await, Err(RepoError::StaleVersion));
}

async fn invite_keys<R: InviteRepo>(repo: &R) {
    repo.insert(&mut invite(1, "bob@example.com", 1))
        .await
        .unwrap();
    assert_eq!(
        repo.insert(&mut invite(1, "eve@example.com", 2)).await,
        Err(RepoError::Duplicate(keys::INVITE_ID))
    );
    assert_eq!(
        repo.insert(&mut invite(2, "bob@example.com", 2)).await,
        Err(RepoError::Duplicate(keys::INVITE_EMAIL))
    );
    assert_eq!(
        repo.insert(&mut invite(3, "eve@example.com", 1)).await,
        Err(RepoError::Duplicate(keys::INVITE_TOKEN))
    );
}

// ---------- staff sessions ----------

/// Runs every `StaffSessionRepo` scenario, each on fresh ports from `make`.
pub async fn staff_sessions<S>(make: impl AsyncFn() -> S)
where
    S: HasStaffSessions + HasSessions + HasUsers + HasRoles,
{
    scenario("staff session: a token's session comes with its user and their role");
    staff_session_found(&make().await).await;
    scenario("staff session: an unknown token is none");
    staff_session_unknown(&make().await).await;
    scenario("staff session: revoked sessions and disabled users come back as stored");
    staff_session_unfiltered(&make().await).await;
    scenario("staff session: a missing user or role is none, the session still found");
    staff_session_missing_parts(&make().await).await;
}

async fn lookup<S: HasStaffSessions>(ports: &S, token: u8) -> Option<StaffSessionRecord> {
    ports
        .staff_sessions()
        .staff_session_by_token(TokenHash::from_bytes([token; 32]))
        .await
        .unwrap()
}

async fn staff_session_found<S>(ports: &S)
where
    S: HasStaffSessions + HasSessions + HasUsers + HasRoles,
{
    let mut r = role(5, "Support");
    ports.roles().insert(&mut r).await.unwrap();
    ports.roles().insert(&mut role(6, "Other")).await.unwrap();
    let mut u = user(1, "ana@example.com", 5);
    ports.users().insert(&mut u).await.unwrap();
    ports
        .users()
        .insert(&mut user(2, "bob@example.com", 6))
        .await
        .unwrap();
    let mut s = session(10, 1, 1);
    ports.sessions().insert(&mut s).await.unwrap();
    ports
        .sessions()
        .insert(&mut session(11, 2, 2))
        .await
        .unwrap();
    assert_eq!(
        lookup(ports, 1).await,
        Some(StaffSessionRecord {
            session: s,
            user: Some(u),
            role: Some(r.clone()),
        })
    );
    // The role as it is now: scopes are read fresh on every request.
    r.set_scopes(&[Scope::ProductsRead]).unwrap();
    ports.roles().update(&mut r).await.unwrap();
    assert_eq!(lookup(ports, 1).await.and_then(|found| found.role), Some(r));
}

async fn staff_session_unknown<S>(ports: &S)
where
    S: HasStaffSessions + HasSessions + HasUsers + HasRoles,
{
    assert_eq!(lookup(ports, 1).await, None);
    ports
        .sessions()
        .insert(&mut session(10, 1, 1))
        .await
        .unwrap();
    assert_eq!(lookup(ports, 2).await, None);
}

async fn staff_session_unfiltered<S>(ports: &S)
where
    S: HasStaffSessions + HasSessions + HasUsers + HasRoles,
{
    let mut r = role(5, "Support");
    ports.roles().insert(&mut r).await.unwrap();
    let mut u = user(1, "ana@example.com", 5);
    ports.users().insert(&mut u).await.unwrap();
    u.set_status(UserStatus::Disabled);
    ports.users().update(&mut u).await.unwrap();
    let mut s = session(10, 1, 1);
    ports.sessions().insert(&mut s).await.unwrap();
    s.revoke(Timestamp::from_unix_micros(1_700_000_000_000_002));
    ports.sessions().update(&mut s).await.unwrap();
    assert_eq!(
        lookup(ports, 1).await,
        Some(StaffSessionRecord {
            session: s,
            user: Some(u),
            role: Some(r),
        })
    );
}

async fn staff_session_missing_parts<S>(ports: &S)
where
    S: HasStaffSessions + HasSessions + HasUsers + HasRoles,
{
    // No user 1 at all.
    let mut orphan = session(10, 1, 1);
    ports.sessions().insert(&mut orphan).await.unwrap();
    assert_eq!(
        lookup(ports, 1).await,
        Some(StaffSessionRecord {
            session: orphan,
            user: None,
            role: None,
        })
    );
    // User 2's role 9 doesn't exist.
    let mut u = user(2, "bob@example.com", 9);
    ports.users().insert(&mut u).await.unwrap();
    let mut s = session(11, 2, 2);
    ports.sessions().insert(&mut s).await.unwrap();
    assert_eq!(
        lookup(ports, 2).await,
        Some(StaffSessionRecord {
            session: s,
            user: Some(u),
            role: None,
        })
    );
}
