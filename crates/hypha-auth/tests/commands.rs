use hypha_auth::{
    ApiKeyKind, CommandError, CreateApiKey, Identity, IdentityError, IdentityRepo, LoginStep,
    MfaMethod, Password, PasswordError, Principal, PrincipalKind, SessionRepo, TokenHash,
    authenticate_api_key, authenticate_session, change_password, check_password, confirm_totp,
    create_api_key, disable_mfa, finish_login, get_api_key, hash_new_password, logout,
    open_session, regenerate_recovery_codes, reset_password, revoke_api_key, start_password_reset,
    start_totp, totp,
};
use hypha_core::{
    Actor, ApiKeyId, AuthError, Clock, Ctx, Email, IdentityId, Scope, Timestamp, UserId,
};
use hypha_testkit::{InMemoryPorts, TestEnv};
use pollster::block_on;
use uuid::Uuid;

const PASSWORD: &str = "correct horse";

fn user_id() -> UserId {
    UserId::from_uuid(Uuid::from_u128(500))
}

fn identity_id() -> IdentityId {
    IdentityId::from_uuid(Uuid::from_u128(501))
}

/// A staff identity for ana@example.com with `PASSWORD`.
fn world() -> (InMemoryPorts, TestEnv) {
    let ports = InMemoryPorts::default();
    let env = TestEnv::default();
    let hash = block_on(hash_new_password(&env, &Password::new(PASSWORD).unwrap())).unwrap();
    let mut identity = Identity::with_password(
        identity_id(),
        Principal::Staff(user_id()),
        Email::new("ana@example.com").unwrap(),
        hash,
    );
    block_on(ports.identities.insert(&mut identity)).unwrap();
    (ports, env)
}

fn identity(ports: &InMemoryPorts) -> Identity {
    block_on(ports.identities.get(identity_id()))
        .unwrap()
        .unwrap()
}

/// The code the authenticator app shows now for `secret` (base32, as `start_totp`
/// returns it), `offset` steps away.
fn code(secret: &str, now: Timestamp, offset: i64) -> String {
    let secret = totp::from_base32(secret).unwrap();
    let step = totp::step_at(now).checked_add_signed(offset).unwrap();
    format!("{:06}", totp::code_at(&secret, step))
}

/// Turns 2FA on; returns the base32 secret and the recovery codes.
fn enable_mfa(ports: &InMemoryPorts, env: &TestEnv) -> (String, Vec<String>) {
    let setup = block_on(start_totp(
        &ports.identities,
        env,
        identity_id(),
        PASSWORD,
        "Shop",
        "ana@example.com",
    ))
    .unwrap();
    let codes = block_on(confirm_totp(
        &ports.identities,
        env,
        identity_id(),
        &code(&setup.secret, env.now(), 0),
    ))
    .unwrap();
    // The next code must come from a later step.
    env.advance(totp::STEP_SECS);
    (setup.secret, codes)
}

fn login(ports: &InMemoryPorts, env: &TestEnv) -> LoginStep {
    let identity = block_on(check_password(
        &ports.identities,
        env,
        PrincipalKind::Staff,
        "ana@example.com",
        PASSWORD,
    ))
    .unwrap();
    block_on(open_session(&ports.sessions, env, &identity)).unwrap()
}

fn token_of(step: &LoginStep) -> String {
    match step {
        LoginStep::Authenticated { token, .. } | LoginStep::MfaRequired { token, .. } => {
            token.expose().to_string()
        }
    }
}

// ---------- passwords and sessions ----------

#[test]
fn the_right_password_opens_an_active_session() {
    let (ports, env) = world();
    let step = login(&ports, &env);
    let LoginStep::Authenticated { session, token } = &step else {
        panic!("expected a session, got {step:?}");
    };
    assert_eq!(session.principal(), Principal::Staff(user_id()));
    assert!(token.expose().starts_with("hypha_sess_"));
    let found = block_on(authenticate_session(&ports.sessions, &env, token.expose())).unwrap();
    assert_eq!(found.id(), session.id());
}

#[test]
fn email_case_does_not_matter_at_login() {
    let (ports, env) = world();
    let found = block_on(check_password(
        &ports.identities,
        &env,
        PrincipalKind::Staff,
        " Ana@Example.COM ",
        PASSWORD,
    ));
    assert_eq!(found.map(|i| i.id()), Ok(identity_id()));
}

#[test]
fn a_wrong_password_or_unknown_email_is_one_error() {
    let (ports, env) = world();
    for (email, password) in [
        ("ana@example.com", "correct horsf"),
        ("bob@example.com", PASSWORD),
        ("not an email", PASSWORD),
        ("ana@example.com", ""),
    ] {
        assert_eq!(
            block_on(check_password(
                &ports.identities,
                &env,
                PrincipalKind::Staff,
                email,
                password
            ))
            .map(|i| i.id()),
            Err(CommandError::InvalidCredentials),
            "{email} {password:?}"
        );
    }
}

#[test]
fn a_session_stops_working_when_it_expires_or_logs_out() {
    let (ports, env) = world();
    let token = token_of(&login(&ports, &env));
    let session = block_on(authenticate_session(&ports.sessions, &env, &token)).unwrap();
    block_on(logout(&ports.sessions, &env, session)).unwrap();
    assert_eq!(
        block_on(authenticate_session(&ports.sessions, &env, &token)).map(|s| s.id()),
        Err(CommandError::InvalidToken)
    );

    let token = token_of(&login(&ports, &env));
    env.advance(hypha_auth::Session::ACTIVE_SECS);
    assert_eq!(
        block_on(authenticate_session(&ports.sessions, &env, &token)).map(|s| s.id()),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn junk_and_other_kinds_of_token_are_not_sessions() {
    let (ports, env) = world();
    for token in ["", "hypha_sess_nope", "Bearer x"] {
        assert_eq!(
            block_on(authenticate_session(&ports.sessions, &env, token)).map(|s| s.id()),
            Err(CommandError::InvalidToken)
        );
    }
    let session = token_of(&login(&ports, &env));
    let as_key = session.replace("hypha_sess_", "hypha_sk_");
    assert_eq!(
        block_on(authenticate_session(&ports.sessions, &env, &as_key)).map(|s| s.id()),
        Err(CommandError::InvalidToken)
    );
}

// ---------- second factor ----------

#[test]
fn with_mfa_the_password_only_opens_a_pending_session() {
    let (ports, env) = world();
    enable_mfa(&ports, &env);
    let step = login(&ports, &env);
    let LoginStep::MfaRequired { methods, .. } = &step else {
        panic!("expected a challenge, got {step:?}");
    };
    assert_eq!(methods, &[MfaMethod::Totp, MfaMethod::RecoveryCode]);
    // The pending token is no session yet.
    assert_eq!(
        block_on(authenticate_session(
            &ports.sessions,
            &env,
            &token_of(&step)
        ))
        .map(|s| s.id()),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn a_totp_code_finishes_the_login_under_a_new_token() {
    let (ports, env) = world();
    let (secret, _) = enable_mfa(&ports, &env);
    let challenge = token_of(&login(&ports, &env));
    let (session, token) = block_on(finish_login(
        &ports,
        &env,
        &challenge,
        &code(&secret, env.now(), 0),
    ))
    .unwrap();
    assert_ne!(token.expose(), challenge);
    assert_eq!(
        block_on(authenticate_session(&ports.sessions, &env, token.expose())).map(|s| s.id()),
        Ok(session.id())
    );
    // The challenge is spent.
    assert_eq!(
        block_on(finish_login(
            &ports,
            &env,
            &challenge,
            &code(&secret, env.now(), 0)
        ))
        .map(|(s, _)| s.id()),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn a_totp_code_works_once() {
    let (ports, env) = world();
    let (secret, _) = enable_mfa(&ports, &env);
    let used = code(&secret, env.now(), 0);
    let challenge = token_of(&login(&ports, &env));
    block_on(finish_login(&ports, &env, &challenge, &used)).unwrap();

    let challenge = token_of(&login(&ports, &env));
    assert_eq!(
        block_on(finish_login(&ports, &env, &challenge, &used)).map(|(s, _)| s.id()),
        Err(CommandError::InvalidMfaCode)
    );
}

#[test]
fn a_recovery_code_finishes_the_login_once() {
    let (ports, env) = world();
    let (_, codes) = enable_mfa(&ports, &env);
    assert_eq!(codes.len(), 10);
    let challenge = token_of(&login(&ports, &env));
    block_on(finish_login(
        &ports,
        &env,
        &challenge,
        &codes[3].to_lowercase(),
    ))
    .unwrap();
    assert_eq!(identity(&ports).recovery_codes_left(), 9);

    let challenge = token_of(&login(&ports, &env));
    assert_eq!(
        block_on(finish_login(&ports, &env, &challenge, &codes[3])).map(|(s, _)| s.id()),
        Err(CommandError::InvalidMfaCode)
    );
}

#[test]
fn the_fifth_wrong_code_ends_the_challenge() {
    let (ports, env) = world();
    let (secret, _) = enable_mfa(&ports, &env);
    let challenge = token_of(&login(&ports, &env));
    for _ in 0..5 {
        assert_eq!(
            block_on(finish_login(&ports, &env, &challenge, "000000")).map(|(s, _)| s.id()),
            Err(CommandError::InvalidMfaCode)
        );
    }
    assert_eq!(
        block_on(finish_login(
            &ports,
            &env,
            &challenge,
            &code(&secret, env.now(), 0)
        ))
        .map(|(s, _)| s.id()),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn a_challenge_expires_after_five_minutes() {
    let (ports, env) = world();
    let (secret, _) = enable_mfa(&ports, &env);
    let challenge = token_of(&login(&ports, &env));
    env.advance(hypha_auth::Session::PENDING_SECS);
    assert_eq!(
        block_on(finish_login(
            &ports,
            &env,
            &challenge,
            &code(&secret, env.now(), 0)
        ))
        .map(|(s, _)| s.id()),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn confirming_needs_the_code_from_the_app() {
    let (ports, env) = world();
    block_on(start_totp(
        &ports.identities,
        &env,
        identity_id(),
        PASSWORD,
        "Shop",
        "ana",
    ))
    .unwrap();
    assert_eq!(
        block_on(confirm_totp(
            &ports.identities,
            &env,
            identity_id(),
            "123456"
        )),
        Err(CommandError::InvalidMfaCode)
    );
    assert!(!identity(&ports).mfa_enabled());
}

#[test]
fn wrong_codes_at_confirmation_count_and_lock() {
    let (ports, env) = world();
    let setup = block_on(start_totp(
        &ports.identities,
        &env,
        identity_id(),
        PASSWORD,
        "Shop",
        "ana",
    ))
    .unwrap();
    for _ in 0..5 {
        assert_eq!(
            block_on(confirm_totp(
                &ports.identities,
                &env,
                identity_id(),
                "000000"
            )),
            Err(CommandError::InvalidMfaCode)
        );
    }
    let until = env.now().plus_secs(Identity::MFA_LOCK_SECS);
    assert_eq!(
        block_on(confirm_totp(
            &ports.identities,
            &env,
            identity_id(),
            &code(&setup.secret, env.now(), 0)
        )),
        Err(CommandError::MfaLocked { until })
    );
    assert!(!identity(&ports).mfa_enabled());
}

#[test]
fn a_used_code_sent_again_is_refused_without_counting() {
    let (ports, env) = world();
    let (secret, _) = enable_mfa(&ports, &env);
    let used = code(&secret, env.now(), 0);
    let challenge = token_of(&login(&ports, &env));
    block_on(finish_login(&ports, &env, &challenge, &used)).unwrap();
    // Sent again (a double submit) more times than the lockout allows.
    for _ in 0..6 {
        assert_eq!(
            block_on(regenerate_recovery_codes(
                &ports.identities,
                &env,
                identity_id(),
                &used
            )),
            Err(CommandError::InvalidMfaCode)
        );
    }
    assert_eq!(identity(&ports).mfa_locked_until(env.now()), None);
}

#[test]
fn the_setup_secret_is_stored_sealed() {
    let (ports, env) = world();
    let setup = block_on(start_totp(
        &ports.identities,
        &env,
        identity_id(),
        PASSWORD,
        "Shop",
        "ana",
    ))
    .unwrap();
    let raw = totp::from_base32(&setup.secret).unwrap();
    let stored = identity(&ports);
    let (sealed, ..) = stored.totp().unwrap();
    assert!(!sealed.as_bytes().windows(raw.len()).any(|w| w == raw));
    assert!(setup.uri.starts_with("otpauth://totp/Shop:ana?secret="));
}

#[test]
fn starting_a_setup_needs_the_password() {
    let (ports, env) = world();
    assert_eq!(
        block_on(start_totp(
            &ports.identities,
            &env,
            identity_id(),
            "wrong",
            "Shop",
            "ana"
        )),
        Err(CommandError::WrongPassword)
    );
    assert_eq!(identity(&ports).totp(), None);
}

#[test]
fn five_wrong_codes_lock_the_second_factor_across_challenges() {
    let (ports, env) = world();
    let (secret, codes) = enable_mfa(&ports, &env);
    let first = token_of(&login(&ports, &env));
    for _ in 0..4 {
        assert_eq!(
            block_on(finish_login(&ports, &env, &first, "000000")).map(|(s, _)| s.id()),
            Err(CommandError::InvalidMfaCode)
        );
    }
    // A new challenge doesn't start the count again: it is the identity's.
    let second = token_of(&login(&ports, &env));
    assert_eq!(
        block_on(finish_login(&ports, &env, &second, "000000")).map(|(s, _)| s.id()),
        Err(CommandError::InvalidMfaCode)
    );
    let until = env.now().plus_secs(Identity::MFA_LOCK_SECS);
    for code in [code(&secret, env.now(), 0), codes[0].clone()] {
        assert_eq!(
            block_on(finish_login(&ports, &env, &second, &code)).map(|(s, _)| s.id()),
            Err(CommandError::MfaLocked { until })
        );
    }
    // The lock also covers the routes for someone already logged in.
    assert_eq!(
        block_on(disable_mfa(
            &ports.identities,
            &env,
            identity_id(),
            &codes[0]
        )),
        Err(CommandError::MfaLocked { until })
    );

    env.advance(Identity::MFA_LOCK_SECS);
    let third = token_of(&login(&ports, &env));
    assert!(
        block_on(finish_login(
            &ports,
            &env,
            &third,
            &code(&secret, env.now(), 0)
        ))
        .is_ok()
    );
}

#[test]
fn wrong_codes_while_logged_in_count_too() {
    let (ports, env) = world();
    let (_, codes) = enable_mfa(&ports, &env);
    for _ in 0..5 {
        assert_eq!(
            block_on(regenerate_recovery_codes(
                &ports.identities,
                &env,
                identity_id(),
                "000000"
            )),
            Err(CommandError::InvalidMfaCode)
        );
    }
    let until = env.now().plus_secs(Identity::MFA_LOCK_SECS);
    assert_eq!(
        block_on(regenerate_recovery_codes(
            &ports.identities,
            &env,
            identity_id(),
            &codes[0]
        )),
        Err(CommandError::MfaLocked { until })
    );
    assert!(identity(&ports).mfa_enabled());
}

#[test]
fn starting_again_once_enabled_is_refused() {
    let (ports, env) = world();
    enable_mfa(&ports, &env);
    assert_eq!(
        block_on(start_totp(
            &ports.identities,
            &env,
            identity_id(),
            PASSWORD,
            "Shop",
            "ana"
        )),
        Err(CommandError::Identity(IdentityError::MfaAlreadyEnabled))
    );
}

#[test]
fn disabling_needs_a_current_code() {
    let (ports, env) = world();
    let (secret, _) = enable_mfa(&ports, &env);
    assert_eq!(
        block_on(disable_mfa(
            &ports.identities,
            &env,
            identity_id(),
            "000000"
        )),
        Err(CommandError::InvalidMfaCode)
    );
    block_on(disable_mfa(
        &ports.identities,
        &env,
        identity_id(),
        &code(&secret, env.now(), 0),
    ))
    .unwrap();
    assert!(!identity(&ports).mfa_enabled());
    assert!(matches!(
        login(&ports, &env),
        LoginStep::Authenticated { .. }
    ));
}

#[test]
fn regenerating_replaces_every_recovery_code() {
    let (ports, env) = world();
    let (_, old) = enable_mfa(&ports, &env);
    let new = block_on(regenerate_recovery_codes(
        &ports.identities,
        &env,
        identity_id(),
        &old[0],
    ))
    .unwrap();
    assert_eq!(new.len(), 10);
    assert!(new.iter().all(|c| !old.contains(c)));
    let challenge = token_of(&login(&ports, &env));
    assert_eq!(
        block_on(finish_login(&ports, &env, &challenge, &old[1])).map(|(s, _)| s.id()),
        Err(CommandError::InvalidMfaCode)
    );
}

// ---------- password changes ----------

#[test]
fn changing_the_password_logs_out_the_other_sessions() {
    let (ports, env) = world();
    let mine = token_of(&login(&ports, &env));
    let other = token_of(&login(&ports, &env));
    let session = block_on(authenticate_session(&ports.sessions, &env, &mine)).unwrap();

    block_on(change_password(
        &ports,
        &env,
        &session,
        PASSWORD,
        "new password",
    ))
    .unwrap();
    assert!(block_on(authenticate_session(&ports.sessions, &env, &mine)).is_ok());
    assert_eq!(
        block_on(authenticate_session(&ports.sessions, &env, &other)).map(|s| s.id()),
        Err(CommandError::InvalidToken)
    );
    assert!(
        block_on(check_password(
            &ports.identities,
            &env,
            PrincipalKind::Staff,
            "ana@example.com",
            "new password"
        ))
        .is_ok()
    );
}

#[test]
fn changing_the_password_needs_the_current_one_and_the_rules() {
    let (ports, env) = world();
    let token = token_of(&login(&ports, &env));
    let session = block_on(authenticate_session(&ports.sessions, &env, &token)).unwrap();
    assert_eq!(
        block_on(change_password(
            &ports,
            &env,
            &session,
            "wrong",
            "new password"
        )),
        Err(CommandError::WrongPassword)
    );
    assert_eq!(
        block_on(change_password(&ports, &env, &session, PASSWORD, "short")),
        Err(CommandError::Password(PasswordError::TooShort { min: 8 }))
    );
}

#[test]
fn a_reset_sets_the_password_once_and_logs_everyone_out() {
    let (ports, env) = world();
    let session = token_of(&login(&ports, &env));
    let (_, _, token) = block_on(start_password_reset(
        &ports,
        &env,
        PrincipalKind::Staff,
        "ana@example.com",
    ))
    .unwrap()
    .unwrap();

    block_on(reset_password(
        &ports,
        &env,
        token.expose(),
        "brand new pass",
    ))
    .unwrap();
    assert_eq!(
        block_on(authenticate_session(&ports.sessions, &env, &session)).map(|s| s.id()),
        Err(CommandError::InvalidToken)
    );
    assert!(
        block_on(check_password(
            &ports.identities,
            &env,
            PrincipalKind::Staff,
            "ana@example.com",
            "brand new pass"
        ))
        .is_ok()
    );
    assert_eq!(
        block_on(reset_password(&ports, &env, token.expose(), "another pass")).map(|i| i.id()),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn a_reset_uses_up_every_other_reset_link() {
    let (ports, env) = world();
    let start = || {
        block_on(start_password_reset(
            &ports,
            &env,
            PrincipalKind::Staff,
            "ana@example.com",
        ))
        .unwrap()
        .unwrap()
        .2
    };
    let first = start();
    // A second link (sent once the first is a few minutes old).
    env.advance(hypha_auth::PasswordReset::RESEND_AFTER_SECS);
    let second = start();
    block_on(reset_password(
        &ports,
        &env,
        first.expose(),
        "brand new pass",
    ))
    .unwrap();
    assert_eq!(
        block_on(reset_password(
            &ports,
            &env,
            second.expose(),
            "another pass"
        ))
        .map(|i| i.id()),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn changing_the_password_uses_up_open_reset_links() {
    let (ports, env) = world();
    let (_, _, link) = block_on(start_password_reset(
        &ports,
        &env,
        PrincipalKind::Staff,
        "ana@example.com",
    ))
    .unwrap()
    .unwrap();
    let token = token_of(&login(&ports, &env));
    let session = block_on(authenticate_session(&ports.sessions, &env, &token)).unwrap();
    block_on(change_password(
        &ports,
        &env,
        &session,
        PASSWORD,
        "new password",
    ))
    .unwrap();
    assert_eq!(
        block_on(reset_password(&ports, &env, link.expose(), "another pass")).map(|i| i.id()),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn one_reset_email_per_account_every_few_minutes() {
    let (ports, env) = world();
    let start = || {
        block_on(start_password_reset(
            &ports,
            &env,
            PrincipalKind::Staff,
            "ana@example.com",
        ))
        .unwrap()
        .map(|(_, reset, _)| reset.id())
    };
    let first = start().unwrap();
    env.advance(hypha_auth::PasswordReset::RESEND_AFTER_SECS - 1);
    assert_eq!(start(), None);
    env.advance(1);
    let second = start().unwrap();
    assert_ne!(first, second);
}

#[test]
fn a_used_link_does_not_hold_back_the_next() {
    let (ports, env) = world();
    let (_, _, token) = block_on(start_password_reset(
        &ports,
        &env,
        PrincipalKind::Staff,
        "ana@example.com",
    ))
    .unwrap()
    .unwrap();
    block_on(reset_password(
        &ports,
        &env,
        token.expose(),
        "brand new pass",
    ))
    .unwrap();
    assert!(
        block_on(start_password_reset(
            &ports,
            &env,
            PrincipalKind::Staff,
            "ana@example.com"
        ))
        .unwrap()
        .is_some()
    );
}

#[test]
fn a_reset_token_expires_after_an_hour() {
    let (ports, env) = world();
    let (_, _, token) = block_on(start_password_reset(
        &ports,
        &env,
        PrincipalKind::Staff,
        "ana@example.com",
    ))
    .unwrap()
    .unwrap();
    env.advance(hypha_auth::PasswordReset::LIFETIME_SECS);
    assert_eq!(
        block_on(reset_password(
            &ports,
            &env,
            token.expose(),
            "brand new pass"
        ))
        .map(|i| i.id()),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn a_reset_for_an_unknown_email_is_nothing() {
    let (ports, env) = world();
    for email in ["bob@example.com", "not an email"] {
        assert_eq!(
            block_on(start_password_reset(
                &ports,
                &env,
                PrincipalKind::Staff,
                email
            ))
            .map(|r| r.is_none()),
            Ok(true)
        );
    }
}

#[test]
fn a_reset_keeps_mfa() {
    let (ports, env) = world();
    enable_mfa(&ports, &env);
    let (_, _, token) = block_on(start_password_reset(
        &ports,
        &env,
        PrincipalKind::Staff,
        "ana@example.com",
    ))
    .unwrap()
    .unwrap();
    block_on(reset_password(
        &ports,
        &env,
        token.expose(),
        "brand new pass",
    ))
    .unwrap();
    assert!(identity(&ports).mfa_enabled());
}

// ---------- API keys ----------

fn admin() -> Ctx {
    Ctx::new(
        Actor::User(user_id()),
        vec![Scope::ApiKeysWrite, Scope::ProductsWrite, Scope::OrdersRead],
    )
}

fn secret_key(scopes: Vec<Scope>) -> CreateApiKey {
    CreateApiKey {
        kind: ApiKeyKind::Secret,
        name: "Importer".to_string(),
        scopes: Some(scopes),
    }
}

#[test]
fn a_secret_key_authenticates_with_its_own_scopes() {
    let (ports, env) = world();
    let (key, token) = block_on(create_api_key(
        &admin(),
        &ports.api_keys,
        &env,
        secret_key(vec![Scope::ProductsWrite]),
    ))
    .unwrap();
    assert!(token.expose().starts_with("hypha_sk_"));
    assert_eq!(key.created_by(), Actor::User(user_id()));
    assert_eq!(key.created_at(), env.now());

    let ctx = block_on(authenticate_api_key(&ports.api_keys, token.expose())).unwrap();
    assert_eq!(ctx.actor(), Actor::ApiKey(key.id()));
    assert!(ctx.has_scope(Scope::ProductsRead));
    assert!(!ctx.has_scope(Scope::OrdersRead));
}

#[test]
fn a_publishable_key_gets_the_storefront_scopes() {
    let (ports, env) = world();
    let ctx = Ctx::new(Actor::System, Scope::ALL.to_vec());
    let input = CreateApiKey {
        kind: ApiKeyKind::Publishable,
        name: "Storefront".to_string(),
        scopes: None,
    };
    let (key, token) = block_on(create_api_key(&ctx, &ports.api_keys, &env, input)).unwrap();
    assert!(token.expose().starts_with("hypha_pk_"));
    assert_eq!(key.scopes(), Scope::STOREFRONT);
}

#[test]
fn nobody_grants_a_scope_they_lack() {
    let (ports, env) = world();
    assert_eq!(
        block_on(create_api_key(
            &admin(),
            &ports.api_keys,
            &env,
            secret_key(vec![Scope::ProductsRead, Scope::CustomersRead]),
        ))
        .map(|(k, _)| k.id()),
        Err(CommandError::CannotGrant(Scope::CustomersRead))
    );
    // Publishable keys too: `admin` lacks carts:write.
    let input = CreateApiKey {
        kind: ApiKeyKind::Publishable,
        name: "Storefront".to_string(),
        scopes: None,
    };
    assert_eq!(
        block_on(create_api_key(&admin(), &ports.api_keys, &env, input)).map(|(k, _)| k.id()),
        Err(CommandError::CannotGrant(Scope::MarketsRead))
    );
}

#[test]
fn creating_a_key_needs_api_keys_write() {
    let (ports, env) = world();
    let ctx = Ctx::new(Actor::User(user_id()), vec![Scope::ProductsWrite]);
    assert_eq!(
        block_on(create_api_key(
            &ctx,
            &ports.api_keys,
            &env,
            secret_key(vec![Scope::ProductsWrite])
        ))
        .map(|(k, _)| k.id()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ApiKeysWrite
        )))
    );
}

#[test]
fn a_revoked_key_stops_working_and_cannot_be_revoked_again() {
    let (ports, env) = world();
    let (key, token) = block_on(create_api_key(
        &admin(),
        &ports.api_keys,
        &env,
        secret_key(vec![Scope::OrdersRead]),
    ))
    .unwrap();
    let revoked = block_on(revoke_api_key(&admin(), &ports.api_keys, &env, key.id())).unwrap();
    assert_eq!(revoked.revoked_at(), Some(env.now()));
    assert_eq!(
        block_on(authenticate_api_key(&ports.api_keys, token.expose())).map(|c| c.actor()),
        Err(CommandError::InvalidToken)
    );
    assert_eq!(
        block_on(revoke_api_key(&admin(), &ports.api_keys, &env, key.id())).map(|k| k.id()),
        Err(CommandError::ApiKeyRevoked(key.id()))
    );
}

#[test]
fn an_unknown_key_is_not_found() {
    let (ports, _) = world();
    let id = ApiKeyId::from_uuid(Uuid::from_u128(9));
    let ctx = Ctx::new(Actor::System, vec![Scope::ApiKeysRead]);
    assert_eq!(
        block_on(get_api_key(&ctx, &ports.api_keys, id)).map(|k| k.id()),
        Err(CommandError::ApiKeyNotFound(id))
    );
}

#[test]
fn only_key_tokens_authenticate_as_keys() {
    let (ports, env) = world();
    let session = token_of(&login(&ports, &env));
    assert_eq!(
        block_on(authenticate_api_key(&ports.api_keys, &session)).map(|c| c.actor()),
        Err(CommandError::InvalidToken)
    );
    let unknown = format!("hypha_sk_{}", "0".repeat(64));
    assert_eq!(
        block_on(authenticate_api_key(&ports.api_keys, &unknown)).map(|c| c.actor()),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn only_hashes_are_stored() {
    let (ports, env) = world();
    let (key, token) = block_on(create_api_key(
        &admin(),
        &ports.api_keys,
        &env,
        secret_key(vec![Scope::OrdersRead]),
    ))
    .unwrap();
    assert_eq!(key.token_hash(), TokenHash::of(token.expose()));
    assert_eq!(key.redacted(), format!("hypha_sk_…{}", token.last_four()));
    let step = login(&ports, &env);
    let session = block_on(ports.sessions.get_by_token(TokenHash::of(&token_of(&step))))
        .unwrap()
        .unwrap();
    assert_eq!(session.created_at(), env.clock.now());
}
