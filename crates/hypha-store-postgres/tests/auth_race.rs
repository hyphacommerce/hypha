//! Concurrent second-factor guesses against a real Postgres, each request on its own
//! connection: guesses sent at once must each count toward the limits, so the challenge
//! ends and the identity locks after five, however many arrive together.
//!
//! Needs `DATABASE_URL` like the contract tests; skips itself without it.

use futures_util::future::join_all;
use hypha_auth::{
    CommandError, Identity, IdentityRepo, LoginStep, Password, Principal, PrincipalKind,
    SessionRepo, TokenHash, check_password, confirm_totp, finish_login, hash_new_password,
    open_session, start_totp, totp,
};
use hypha_core::{Email, IdentityId, UserId};
use hypha_store_postgres::{CORE, PgStore, migrate};
use hypha_testkit::TestEnv;
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
    let store = PgStore::new(client);
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
        stores.push(PgStore::new(connect(&url, schema).await));
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
