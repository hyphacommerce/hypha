//! Concurrent auth requests against a real Postgres, each on its own connection:
//! second-factor guesses sent at once must each count toward the limits, and two owners
//! removing each other at once must leave one.
//!
//! Needs `DATABASE_URL` like the contract tests; skips itself without it.

use futures_util::future::join_all;
use hypha_auth::{
    CommandError, Identity, IdentityRepo, LoginStep, Password, Principal, PrincipalKind,
    SessionRepo, TokenHash, check_password, confirm_totp, finish_login, hash_new_password,
    open_session, start_password_reset, start_totp, totp,
};
use hypha_core::{Actor, Ctx, Email, IdentityId, Scope, UserId};
use hypha_store_postgres::{CORE, PgStore, migrate};
use hypha_testkit::TestEnv;
use hypha_users::{NewOwner, UpdateUser, User, UserRepo, UserStatus, bootstrap_owner, update_user};
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

const PASSWORD: &str = "correct horse";

async fn connect(url: &str, schema: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("connect to DATABASE_URL");
    tokio::spawn(connection);
    client
        .batch_execute(&format!("SET search_path TO {schema}"))
        .await
        .expect("set search_path");
    client
}

#[tokio::test]
async fn parallel_wrong_codes_all_count() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping");
        return;
    };
    let schema = "race_mfa_guesses";
    let mut client = connect(&url, "public").await;
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; \
             SET search_path TO {schema};"
        ))
        .await
        .expect("reset schema");
    migrate(&mut client, &[CORE]).await.expect("migrate");
    let store = PgStore::new(client, clock());
    let env = TestEnv::default();

    // An identity with 2FA on, and a pending login.
    let id = IdentityId::from_uuid(Uuid::from_u128(1));
    let hash = hash_new_password(&env, &Password::new(PASSWORD).unwrap())
        .await
        .unwrap();
    let mut identity = Identity::with_password(
        id,
        Principal::Staff(UserId::from_uuid(Uuid::from_u128(2))),
        Email::new("ana@example.com").unwrap(),
        hash,
    );
    IdentityRepo::insert(&store, &mut identity).await.unwrap();
    let setup = start_totp(&store, &env, id, PASSWORD, "Shop", "ana")
        .await
        .unwrap();
    let secret = totp::from_base32(&setup.secret).unwrap();
    let right = format!("{:06}", totp::code_at(&secret, totp::step_at(env.now())));
    confirm_totp(&store, &env, id, &right).await.unwrap();
    env.advance(totp::STEP_SECS);
    let identity = check_password(
        &store,
        &env,
        PrincipalKind::Staff,
        "ana@example.com",
        PASSWORD,
    )
    .await
    .unwrap();
    let LoginStep::MfaRequired { token, .. } = open_session(&store, &env, &identity).await.unwrap()
    else {
        panic!("expected a challenge");
    };
    let challenge = token.expose().to_string();

    // 40 different wrong codes at once, each on its own connection.
    let current = totp::step_at(env.now());
    let valid: Vec<String> = (current - 1..=current + 1)
        .map(|s| format!("{:06}", totp::code_at(&secret, s)))
        .collect();
    let guesses: Vec<String> = (0..)
        .map(|n| format!("{n:06}"))
        .filter(|c| !valid.contains(c))
        .take(40)
        .collect();
    let mut stores = Vec::new();
    for _ in &guesses {
        stores.push(PgStore::new(connect(&url, schema).await, clock()));
    }
    let results = join_all(
        stores
            .iter()
            .zip(&guesses)
            .map(|(store, code)| finish_login(store, &env, &challenge, code)),
    )
    .await;

    let wrong = results
        .iter()
        .filter(|r| matches!(r, Err(CommandError::InvalidMfaCode)))
        .count();
    assert!(results.iter().all(|r| r.is_err()), "a wrong code logged in");
    assert!(
        wrong <= 5,
        "{wrong} wrong codes were checked and reported, at most 5 may be"
    );

    // At least five were counted: the challenge is over and the identity locked.
    let session = SessionRepo::get_by_token(&store, TokenHash::of(&challenge))
        .await
        .unwrap()
        .unwrap();
    assert!(
        session.revoked_at().is_some(),
        "the challenge survived {} guesses",
        guesses.len()
    );
    let identity = IdentityRepo::get(&store, id).await.unwrap().unwrap();
    assert!(identity.mfa_locked_until(env.now()).is_some());
}

#[tokio::test]
async fn two_owners_disabling_each_other_leave_one_active() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping");
        return;
    };
    let schema = "race_last_owner";
    let mut client = connect(&url, "public").await;
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; \
             SET search_path TO {schema};"
        ))
        .await
        .expect("reset schema");
    migrate(&mut client, &[CORE]).await.expect("migrate");
    let store = PgStore::new(client, clock());
    let env = TestEnv::default();
    let ana = bootstrap_owner(
        &Ctx::system(),
        &store,
        &env,
        NewOwner {
            email: "ana@example.com".to_string(),
            name: "Ana".to_string(),
            password: PASSWORD.to_string(),
        },
    )
    .await
    .unwrap()
    .unwrap();
    let mut bob = User::new(
        UserId::from_uuid(Uuid::from_u128(900)),
        Email::new("bob@example.com").unwrap(),
        "Bob",
        ana.role(),
        env.now(),
    )
    .unwrap();
    UserRepo::insert(&store, &mut bob).await.unwrap();

    // Many rounds, since one race may happen to run one after the other.
    for _ in 0..10 {
        let (a, b) = (
            PgStore::new(connect(&url, schema).await, clock()),
            PgStore::new(connect(&url, schema).await, clock()),
        );
        let disable = || UpdateUser {
            status: Some(UserStatus::Disabled),
            ..UpdateUser::default()
        };
        let ctx = |user: &User| Ctx::new(Actor::User(user.id()), Scope::ALL.to_vec());
        let (ana_ctx, bob_ctx) = (ctx(&ana), ctx(&bob));
        let (first, second) = tokio::join!(
            update_user(&ana_ctx, &a, &env, bob.id(), disable()),
            update_user(&bob_ctx, &b, &env, ana.id(), disable()),
        );
        let active = UserRepo::count_active_with_role(&store, ana.role())
            .await
            .unwrap();
        assert_eq!(active, 1, "{first:?} {second:?}");
        assert!(first.is_ok() != second.is_ok(), "{first:?} {second:?}");

        // Enable both again for the next round.
        for id in [ana.id(), bob.id()] {
            let enable = UpdateUser {
                status: Some(UserStatus::Active),
                ..UpdateUser::default()
            };
            update_user(&Ctx::system(), &store, &env, id, enable)
                .await
                .unwrap();
        }
    }
}

#[tokio::test]
async fn parallel_reset_requests_make_one_link() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping");
        return;
    };
    let schema = "race_reset_requests";
    let mut client = connect(&url, "public").await;
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; \
             SET search_path TO {schema};"
        ))
        .await
        .expect("reset schema");
    migrate(&mut client, &[CORE]).await.expect("migrate");
    let store = PgStore::new(client, clock());
    let env = TestEnv::default();
    let hash = hash_new_password(&env, &Password::new(PASSWORD).unwrap())
        .await
        .unwrap();
    let mut identity = Identity::with_password(
        IdentityId::from_uuid(Uuid::from_u128(1)),
        Principal::Staff(UserId::from_uuid(Uuid::from_u128(2))),
        Email::new("ana@example.com").unwrap(),
        hash,
    );
    IdentityRepo::insert(&store, &mut identity).await.unwrap();

    let mut stores = Vec::new();
    for _ in 0..8 {
        stores.push(PgStore::new(connect(&url, schema).await, clock()));
    }
    let results = join_all(
        stores
            .iter()
            .map(|s| start_password_reset(s, &env, PrincipalKind::Staff, "ana@example.com")),
    )
    .await;
    let made = results
        .into_iter()
        .map(|r| r.unwrap())
        .filter(Option::is_some)
        .count();
    assert_eq!(made, 1, "parallel requests made {made} links");
}

/// Writes are stamped with a fixed time in these tests.
fn clock() -> std::sync::Arc<hypha_testkit::FixedClock> {
    std::sync::Arc::new(hypha_testkit::FixedClock::default())
}
