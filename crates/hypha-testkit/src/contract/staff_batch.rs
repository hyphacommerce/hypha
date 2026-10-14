use hypha_auth::{
    HasIdentities, HasPasswordResets, HasSessions, IdentityRepo, PasswordResetRepo, Sealed,
    SessionRepo, StageIdentities, StagePasswordResets, StageSessions, TokenHash, keys as auth_keys,
};
use hypha_core::{HasBatch, RepoError, SessionId, Timestamp, Version, Versioned};
use hypha_users::{
    HasInvites, HasRoles, HasUsers, InviteRepo, Role, RoleRepo, StageInvites, StageRoles,
    StageUsers, UserRepo, keys as user_keys,
};

use super::{
    auth::{identity, reset, session},
    scenario,
    users::{invite, role_id, user},
};

/// Runs the `HasBatch` scenarios of the staff flows (design note 0008): accepting an
/// invite, the owner's first start, a password reset.
pub async fn staff_batch<S>(make: impl AsyncFn() -> S)
where
    S: HasBatch
        + HasUsers
        + HasRoles
        + HasInvites
        + HasIdentities
        + HasSessions
        + HasPasswordResets,
    S::Batch: StageUsers
        + StageRoles
        + StageInvites
        + StageIdentities
        + StageSessions
        + StagePasswordResets,
{
    scenario("staff batch: accepting an invite writes user, identity, session and invite");
    accept_invite(&make().await).await;
    scenario("staff batch: a taken login writes nothing");
    taken_login_writes_nothing(&make().await).await;
    scenario("staff batch: the owner's role, user and identity commit together");
    bootstrap(&make().await).await;
    scenario("staff batch: a user and role update commit together; a stale role writes nothing");
    update_user_and_role(&make().await).await;
    scenario("staff batch: revoking sessions spares `keep`, other identities and old revocations");
    revoke_sessions(&make().await).await;
    scenario("staff batch: a reset updates identity and reset; a stale reset writes nothing");
    password_reset(&make().await).await;
    scenario("staff batch: using up resets spares other identities and used ones");
    use_up_resets(&make().await).await;
}

async fn use_up_resets<S>(ports: &S)
where
    S: HasBatch + HasPasswordResets,
    S::Batch: StagePasswordResets,
{
    let early = Timestamp::from_unix_micros(1);
    let at = Timestamp::from_unix_micros(100);
    let mut presented = reset(1, 1, 1);
    let mut other = reset(2, 1, 2);
    let mut used = reset(3, 1, 3);
    let mut someone_else = reset(4, 2, 4);
    used.mark_used(early);
    for r in [&mut presented, &mut other, &mut used, &mut someone_else] {
        ports.password_resets().insert(r).await.unwrap();
    }
    let later = Timestamp::from_unix_micros(50);
    presented.mark_used(later);
    let mut batch = ports.batch();
    batch.update_password_reset(&mut presented);
    batch.use_up_password_resets(presented.identity(), at);
    ports.commit(batch).await.unwrap();

    let get = async |n: u8| {
        ports
            .password_resets()
            .get_by_token(TokenHash::from_bytes([n; 32]))
            .await
            .unwrap()
            .unwrap()
    };
    assert_eq!(get(1).await, presented);
    assert_eq!(get(2).await.used_at(), Some(at));
    assert_eq!(get(2).await.version(), Version::new(2));
    assert_eq!(get(3).await, used);
    assert_eq!(get(4).await, someone_else);
}

async fn accept_invite<S>(ports: &S)
where
    S: HasBatch + HasUsers + HasInvites + HasIdentities + HasSessions,
    S::Batch: StageUsers + StageInvites + StageIdentities + StageSessions,
{
    let mut inv = invite(1, "bob@example.com", 1);
    ports.invites().insert(&mut inv).await.unwrap();
    let mut u = user(1, "bob@example.com", 1);
    let mut i = identity(1, "bob@example.com");
    let mut s = session(1, 1, 1);
    inv.accept(Timestamp::from_unix_micros(9));

    let mut batch = ports.batch();
    batch.update_invite(&mut inv);
    batch.insert_user(&mut u);
    batch.insert_identity(&mut i);
    batch.insert_session(&mut s);
    assert_eq!(
        (inv.version(), u.version(), i.version(), s.version()),
        (
            Version::new(2),
            Version::new(1),
            Version::new(1),
            Version::new(1)
        )
    );
    ports.commit(batch).await.unwrap();

    assert_eq!(
        ports.invites().get_by_email(inv.email()).await,
        Ok(Some(inv))
    );
    assert_eq!(ports.users().get(u.id()).await, Ok(Some(u)));
    assert_eq!(ports.identities().get(i.id()).await, Ok(Some(i)));
    assert_eq!(ports.sessions().get(s.id()).await, Ok(Some(s)));
}

async fn taken_login_writes_nothing<S>(ports: &S)
where
    S: HasBatch + HasUsers + HasInvites + HasIdentities + HasSessions,
    S::Batch: StageUsers + StageInvites + StageIdentities + StageSessions,
{
    ports
        .identities()
        .insert(&mut identity(9, "bob@example.com"))
        .await
        .unwrap();
    let mut inv = invite(1, "bob@example.com", 1);
    ports.invites().insert(&mut inv).await.unwrap();
    let stored = inv.clone();
    let mut u = user(1, "bob@example.com", 1);
    let mut s = session(1, 1, 1);
    inv.accept(Timestamp::from_unix_micros(9));

    let mut batch = ports.batch();
    batch.update_invite(&mut inv);
    batch.insert_user(&mut u);
    batch.insert_identity(&mut identity(1, "bob@example.com"));
    batch.insert_session(&mut s);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(auth_keys::IDENTITY_LOGIN))
    );
    assert_eq!(
        ports.invites().get_by_email(inv.email()).await,
        Ok(Some(stored))
    );
    assert_eq!(ports.users().get(u.id()).await, Ok(None));
    assert_eq!(ports.sessions().get(s.id()).await, Ok(None));
}

async fn bootstrap<S>(ports: &S)
where
    S: HasBatch + HasUsers + HasRoles + HasIdentities,
    S::Batch: StageUsers + StageRoles + StageIdentities,
{
    let mut owner = Role::owner(role_id(1));
    let mut u = user(1, "ana@example.com", 1);
    let mut i = identity(1, "ana@example.com");
    let mut batch = ports.batch();
    batch.insert_role(&mut owner);
    batch.insert_user(&mut u);
    batch.insert_identity(&mut i);
    ports.commit(batch).await.unwrap();
    assert_eq!(ports.roles().owner().await, Ok(Some(owner)));
    assert_eq!(ports.users().get(u.id()).await, Ok(Some(u)));

    // A second owner role writes nothing.
    let mut batch = ports.batch();
    batch.insert_role(&mut Role::owner(role_id(2)));
    batch.insert_user(&mut user(2, "bob@example.com", 2));
    let result = ports.commit(batch).await;
    assert!(
        matches!(
            result,
            Err(RepoError::Duplicate(
                user_keys::ROLE_OWNER | user_keys::ROLE_NAME
            ))
        ),
        "{result:?}"
    );
    assert_eq!(ports.users().any().await, Ok(true));
    let bob = hypha_core::Email::new("bob@example.com").unwrap();
    assert_eq!(ports.users().get_by_email(&bob).await, Ok(None));
}

async fn update_user_and_role<S>(ports: &S)
where
    S: HasBatch + HasUsers + HasRoles,
    S::Batch: StageUsers + StageRoles,
{
    let mut owner = Role::owner(role_id(1));
    ports.roles().insert(&mut owner).await.unwrap();
    let mut u = user(1, "ana@example.com", 1);
    ports.users().insert(&mut u).await.unwrap();
    let stale_role = owner.clone();

    u.set_status(hypha_users::UserStatus::Disabled);
    let mut batch = ports.batch();
    batch.update_user(&mut u);
    batch.update_role(&mut owner);
    assert_eq!(
        (u.version(), owner.version()),
        (Version::new(2), Version::new(2))
    );
    ports.commit(batch).await.unwrap();
    assert_eq!(ports.users().get(u.id()).await, Ok(Some(u.clone())));
    assert_eq!(ports.roles().get(owner.id()).await, Ok(Some(owner)));

    // A batch claiming the role at its old version writes nothing, not even the user.
    let before = u.clone();
    u.rename("Someone Else").unwrap();
    let mut batch = ports.batch();
    batch.update_user(&mut u);
    batch.update_role(&mut stale_role.clone());
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.users().get(u.id()).await, Ok(Some(before)));
}

async fn revoke_sessions<S>(ports: &S)
where
    S: HasBatch + HasSessions,
    S::Batch: StageSessions,
{
    let early = Timestamp::from_unix_micros(1);
    let at = Timestamp::from_unix_micros(100);
    let mut kept = session(1, 1, 1);
    let mut revoked = session(2, 1, 2);
    let mut already = session(3, 1, 3);
    let mut other = session(4, 2, 4);
    already.revoke(early);
    for s in [&mut kept, &mut revoked, &mut already, &mut other] {
        ports.sessions().insert(s).await.unwrap();
    }

    let mut batch = ports.batch();
    batch.revoke_sessions(kept.identity(), Some(kept.id()), at);
    ports.commit(batch).await.unwrap();

    let get = async |id: SessionId| ports.sessions().get(id).await.unwrap().unwrap();
    assert_eq!(get(kept.id()).await, kept);
    assert_eq!(get(other.id()).await, other);
    assert_eq!(get(already.id()).await, already);
    let after = get(revoked.id()).await;
    assert_eq!(after.revoked_at(), Some(at));
    assert_eq!(after.version(), Version::new(2));

    // Without `keep`, every live session of the identity goes.
    let mut batch = ports.batch();
    batch.revoke_sessions(kept.identity(), None, at);
    ports.commit(batch).await.unwrap();
    assert_eq!(get(kept.id()).await.revoked_at(), Some(at));
}

async fn password_reset<S>(ports: &S)
where
    S: HasBatch + HasIdentities + HasPasswordResets,
    S::Batch: StageIdentities + StagePasswordResets,
{
    let mut i = identity(1, "ana@example.com");
    ports.identities().insert(&mut i).await.unwrap();
    let mut r = reset(1, 1, 1);
    ports.password_resets().insert(&mut r).await.unwrap();
    let mut stale = r.clone();

    i.start_totp(Sealed::from_bytes(vec![1])).unwrap();
    r.mark_used(Timestamp::from_unix_micros(5));
    let mut batch = ports.batch();
    batch.update_identity(&mut i);
    batch.update_password_reset(&mut r);
    ports.commit(batch).await.unwrap();
    assert_eq!(ports.identities().get(i.id()).await, Ok(Some(i.clone())));
    assert_eq!(
        ports
            .password_resets()
            .get_by_token(TokenHash::from_bytes([1; 32]))
            .await,
        Ok(Some(r))
    );

    // Used twice at once: the second batch is stale and writes nothing.
    let before = i.clone();
    i.start_totp(Sealed::from_bytes(vec![2])).unwrap();
    stale.mark_used(Timestamp::from_unix_micros(6));
    let mut batch = ports.batch();
    batch.update_identity(&mut i);
    batch.update_password_reset(&mut stale);
    assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    assert_eq!(ports.identities().get(before.id()).await, Ok(Some(before)));
}
