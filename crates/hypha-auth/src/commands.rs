//! Authentication building blocks (design note 0008). `hypha-users` composes them into
//! the staff flows (login, invites); hosts call the API key ones directly.
//!
//! The commands acting on "my own" identity (`start_totp`, `change_password`, …) take
//! the identity or session the host resolved from the request's token, never one from
//! the request body: that is their authorization.

use hypha_core::{
    Actor, ApiKeyId, AuthError, Clock, Ctx, Email, ErrorCode, HasBatch, IdGenerator, IdentityId,
    PasswordResetId, Random, RepoError, Scope, SessionId,
};
use thiserror::Error;

use crate::{
    ApiKey, ApiKeyError, Identity, IdentityError, PasswordReset, Session,
    api_key::{ApiKeyKind, ApiKeyParts},
    identity::{CredentialKind, PrincipalKind},
    password::{HashError, Password, PasswordError, PasswordHasher},
    recovery,
    repo::{
        ApiKeyRepo, HasIdentities, HasPasswordResets, HasSessions, IdentityRepo, PasswordResetRepo,
        SessionRepo, StageIdentities, StagePasswordResets, StageSessions,
    },
    secret_box::{SealError, SecretBox},
    token::{NewToken, TokenHash, TokenKind},
    totp,
};

/// The non-storage ports auth commands use, as one bound (design note 0008, section 8).
/// Like `hypha_cart::CartPorts`, the blanket impl means nobody implements it by hand.
pub trait AuthEnv: IdGenerator + Clock + Random + PasswordHasher + SecretBox {}

impl<T: IdGenerator + Clock + Random + PasswordHasher + SecretBox> AuthEnv for T {}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Hash(#[from] HashError),
    #[error(transparent)]
    Seal(#[from] SealError),
    #[error(transparent)]
    Password(#[from] PasswordError),
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error(transparent)]
    ApiKey(#[from] ApiKeyError),
    /// Wrong email or password, or a disabled account: deliberately one answer.
    #[error("wrong email or password")]
    InvalidCredentials,
    /// A token that is malformed, unknown, expired or revoked: one answer for all.
    #[error("the token is invalid or expired")]
    InvalidToken,
    #[error("wrong or already used code")]
    InvalidMfaCode,
    #[error("the current password is wrong")]
    WrongPassword,
    #[error("api key {0} not found")]
    ApiKeyNotFound(ApiKeyId),
    #[error("api key {0} is already revoked")]
    ApiKeyRevoked(ApiKeyId),
    #[error("identity {0} not found")]
    IdentityNotFound(IdentityId),
    /// Granting a scope the caller doesn't hold (design note 0008, section 4).
    #[error("you can't grant {0}, which you don't have")]
    CannotGrant(Scope),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Hash(_) | CommandError::Seal(_) => "internal_error",
            CommandError::Password(e) => e.code(),
            CommandError::Identity(e) => e.code(),
            CommandError::ApiKey(e) => e.code(),
            CommandError::InvalidCredentials => "invalid_credentials",
            CommandError::InvalidToken => "invalid_token",
            CommandError::InvalidMfaCode => "invalid_mfa_code",
            CommandError::WrongPassword => "wrong_password",
            CommandError::ApiKeyNotFound(_) => "api_key_not_found",
            CommandError::ApiKeyRevoked(_) => "api_key_revoked",
            CommandError::IdentityNotFound(_) => "identity_not_found",
            CommandError::CannotGrant(_) => "not_allowed_to_grant",
        }
    }
}

/// Fails unless `ctx` holds every scope in `scopes`, so nobody hands out more than they
/// have (a role, an invite, a key).
pub fn require_grantable(ctx: &Ctx, scopes: &[Scope]) -> Result<(), CommandError> {
    match scopes.iter().find(|s| !ctx.has_scope(**s)) {
        Some(missing) => Err(CommandError::CannotGrant(*missing)),
        None => Ok(()),
    }
}

/// 16 random bytes, for a password salt.
fn salt(random: &impl Random) -> [u8; 16] {
    let mut salt = [0u8; 16];
    random.fill(&mut salt);
    salt
}

/// Hashes a new password.
pub async fn hash_new_password(
    env: &impl AuthEnv,
    password: &Password,
) -> Result<crate::PasswordHash, CommandError> {
    Ok(env.hash_password(password, salt(env)).await?)
}

// ---------- login ----------

/// Second factors a pending login accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MfaMethod {
    Totp,
    RecoveryCode,
}

impl MfaMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            MfaMethod::Totp => "totp",
            MfaMethod::RecoveryCode => "recovery_code",
        }
    }
}

/// Where a login stands after the password (design note 0008, section 3).
#[derive(Debug)]
pub enum LoginStep {
    /// Logged in.
    Authenticated { session: Session, token: NewToken },
    /// A second factor is owed: send `token` back with a code to `finish_login`.
    MfaRequired {
        session: Session,
        token: NewToken,
        methods: Vec<MfaMethod>,
    },
}

/// The identity behind `email` and `password`, or `InvalidCredentials`. An unknown
/// email costs a hash too, so response times don't reveal which emails exist.
pub async fn check_password<R: IdentityRepo>(
    identities: &R,
    env: &impl AuthEnv,
    kind: PrincipalKind,
    email: &str,
    password: &str,
) -> Result<Identity, CommandError> {
    let Some(password) = Password::attempt(password) else {
        return Err(CommandError::InvalidCredentials);
    };
    let found = match Email::new(email) {
        Ok(email) => {
            identities
                .find_login(kind, CredentialKind::Password, email.as_str())
                .await?
        }
        Err(_) => None,
    };
    let Some((identity, hash)) = found.and_then(|i| {
        let hash = i.password().map(|(_, hash)| hash.clone());
        hash.map(|hash| (i, hash))
    }) else {
        // Same work as a real check; the result doesn't matter.
        env.hash_password(&password, [0; 16]).await?;
        return Err(CommandError::InvalidCredentials);
    };
    if env.verify_password(&password, &hash).await? {
        Ok(identity)
    } else {
        Err(CommandError::InvalidCredentials)
    }
}

/// Opens a session for an identity whose first factor checked out: active, or pending
/// if it has a second factor.
pub async fn open_session<R: SessionRepo>(
    sessions: &R,
    env: &impl AuthEnv,
    identity: &Identity,
) -> Result<LoginStep, CommandError> {
    let id = SessionId::from_uuid(env.new_uuid());
    let token = NewToken::generate(TokenKind::Session, env);
    let now = env.now();
    if identity.mfa_enabled() {
        let mut session =
            Session::pending(id, identity.id(), identity.principal(), token.hash(), now);
        sessions.insert(&mut session).await?;
        let mut methods = vec![MfaMethod::Totp];
        if identity.recovery_codes_left() > 0 {
            methods.push(MfaMethod::RecoveryCode);
        }
        Ok(LoginStep::MfaRequired {
            session,
            token,
            methods,
        })
    } else {
        let mut session = Session::active(
            id,
            identity.id(),
            identity.principal(),
            token.hash(),
            now,
            None,
        );
        sessions.insert(&mut session).await?;
        Ok(LoginStep::Authenticated { session, token })
    }
}

/// A new active session for `identity`, staged rather than written (accepting an invite
/// logs the new user in, in the same batch).
pub fn new_active_session(identity: &Identity, env: &impl AuthEnv) -> (Session, NewToken) {
    let token = NewToken::generate(TokenKind::Session, env);
    let session = Session::active(
        SessionId::from_uuid(env.new_uuid()),
        identity.id(),
        identity.principal(),
        token.hash(),
        env.now(),
        None,
    );
    (session, token)
}

/// Checks a second-factor `code` against `identity` and uses it up: a 6-digit TOTP
/// code (its step is recorded) or a recovery code (removed). `false` if neither.
fn use_second_factor(
    identity: &mut Identity,
    env: &impl AuthEnv,
    code: &str,
) -> Result<bool, CommandError> {
    let code = code.trim();
    if code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()) {
        let Some((sealed, true, last_step)) = identity.totp() else {
            return Ok(false);
        };
        let secret = env.open(sealed, identity.id().to_string().as_bytes())?;
        return match totp::verify(&secret, code, env.now(), last_step) {
            Some(step) => {
                identity.record_totp_step(step)?;
                Ok(true)
            }
            None => Ok(false),
        };
    }
    Ok(recovery::hash(code).is_some_and(|hash| identity.use_recovery_code(hash)))
}

/// Finishes a login that owes a second factor: `challenge` is the pending session's
/// token, `code` a TOTP or recovery code. Returns the session under a new token. Each
/// wrong code counts, and the fifth ends the pending session.
pub async fn finish_login<S>(
    ports: &S,
    env: &impl AuthEnv,
    challenge: &str,
    code: &str,
) -> Result<(Session, NewToken), CommandError>
where
    S: HasIdentities + HasSessions + HasBatch<Batch: StageIdentities + StageSessions>,
{
    let now = env.now();
    if TokenKind::of(challenge) != Some(TokenKind::Session) {
        return Err(CommandError::InvalidToken);
    }
    let mut session = ports
        .sessions()
        .get_by_token(TokenHash::of(challenge))
        .await?
        .filter(|s| s.is_pending(now))
        .ok_or(CommandError::InvalidToken)?;
    let mut identity = ports
        .identities()
        .get(session.identity())
        .await?
        .ok_or(CommandError::IdentityNotFound(session.identity()))?;

    if !use_second_factor(&mut identity, env, code)? {
        session.record_failed_attempt(now);
        ports.sessions().update(&mut session).await?;
        return Err(CommandError::InvalidMfaCode);
    }
    let token = NewToken::generate(TokenKind::Session, env);
    session.activate(token.hash(), now);
    let mut batch = ports.batch();
    batch.update_identity(&mut identity);
    batch.update_session(&mut session);
    // A `StaleVersion` here means another request used a code on this identity or
    // session at the same moment; the client logs in again.
    ports.commit(batch).await?;
    Ok((session, token))
}

/// The active session behind `token`, or `InvalidToken`.
pub async fn authenticate_session<R: SessionRepo>(
    sessions: &R,
    clock: &impl Clock,
    token: &str,
) -> Result<Session, CommandError> {
    if TokenKind::of(token) != Some(TokenKind::Session) {
        return Err(CommandError::InvalidToken);
    }
    sessions
        .get_by_token(TokenHash::of(token))
        .await?
        .filter(|s| s.is_active(clock.now()))
        .ok_or(CommandError::InvalidToken)
}

/// Ends `session`.
pub async fn logout<R: SessionRepo>(
    sessions: &R,
    clock: &impl Clock,
    mut session: Session,
) -> Result<(), CommandError> {
    session.revoke(clock.now());
    sessions.update(&mut session).await?;
    Ok(())
}

// ---------- passwords ----------

async fn load_identity<R: IdentityRepo>(
    identities: &R,
    id: IdentityId,
) -> Result<Identity, CommandError> {
    identities
        .get(id)
        .await?
        .ok_or(CommandError::IdentityNotFound(id))
}

/// Changes the password of `session`'s identity and logs out its other sessions.
pub async fn change_password<S>(
    ports: &S,
    env: &impl AuthEnv,
    session: &Session,
    current: &str,
    new: &str,
) -> Result<(), CommandError>
where
    S: HasIdentities + HasBatch<Batch: StageIdentities + StageSessions>,
{
    let new = Password::new(new)?;
    let mut identity = load_identity(ports.identities(), session.identity()).await?;
    let hash = identity
        .password()
        .map(|(_, hash)| hash.clone())
        .ok_or(IdentityError::NoPassword)?;
    let current = Password::attempt(current).ok_or(CommandError::WrongPassword)?;
    if !env.verify_password(&current, &hash).await? {
        return Err(CommandError::WrongPassword);
    }
    identity.set_password_hash(hash_new_password(env, &new).await?)?;
    let mut batch = ports.batch();
    batch.update_identity(&mut identity);
    batch.revoke_sessions(identity.id(), Some(session.id()), env.now());
    ports.commit(batch).await?;
    Ok(())
}

/// A reset for the identity of `kind` that logs in with `email`, or `None` if there is
/// none (the caller answers the same either way). The caller emails the token.
pub async fn start_password_reset<S>(
    ports: &S,
    env: &impl AuthEnv,
    kind: PrincipalKind,
    email: &str,
) -> Result<Option<(Identity, PasswordReset, NewToken)>, CommandError>
where
    S: HasIdentities + HasPasswordResets,
{
    let Ok(email) = Email::new(email) else {
        return Ok(None);
    };
    let Some(identity) = ports
        .identities()
        .find_login(kind, CredentialKind::Password, email.as_str())
        .await?
    else {
        return Ok(None);
    };
    let token = NewToken::generate(TokenKind::PasswordReset, env);
    let mut reset = PasswordReset::new(
        PasswordResetId::from_uuid(env.new_uuid()),
        identity.id(),
        token.hash(),
        env.now(),
    );
    ports.password_resets().insert(&mut reset).await?;
    Ok(Some((identity, reset, token)))
}

/// Sets a new password with an emailed reset token, uses the token up and logs out
/// every session of the identity, in one batch.
pub async fn reset_password<S>(
    ports: &S,
    env: &impl AuthEnv,
    token: &str,
    new: &str,
) -> Result<Identity, CommandError>
where
    S: HasIdentities
        + HasPasswordResets
        + HasBatch<Batch: StageIdentities + StageSessions + StagePasswordResets>,
{
    let now = env.now();
    if TokenKind::of(token) != Some(TokenKind::PasswordReset) {
        return Err(CommandError::InvalidToken);
    }
    let mut reset = ports
        .password_resets()
        .get_by_token(TokenHash::of(token))
        .await?
        .filter(|r| r.is_usable(now))
        .ok_or(CommandError::InvalidToken)?;
    let new = Password::new(new)?;
    let mut identity = load_identity(ports.identities(), reset.identity()).await?;
    identity.set_password_hash(hash_new_password(env, &new).await?)?;
    reset.mark_used(now);
    let mut batch = ports.batch();
    batch.update_identity(&mut identity);
    batch.update_password_reset(&mut reset);
    batch.revoke_sessions(identity.id(), None, now);
    // Two resets with the same token race on the reset's version: one wins.
    ports.commit(batch).await?;
    Ok(identity)
}

// ---------- second factor ----------

/// What an authenticator app needs: the secret to type in, or the URI as a QR code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotpSetup {
    pub secret: String,
    pub uri: String,
}

/// Starts setting up an authenticator for `identity` (replacing an unconfirmed setup).
/// `issuer` and `account` label the entry in the app: the store name and the email.
pub async fn start_totp<R: IdentityRepo>(
    identities: &R,
    env: &impl AuthEnv,
    identity: IdentityId,
    issuer: &str,
    account: &str,
) -> Result<TotpSetup, CommandError> {
    let mut identity = load_identity(identities, identity).await?;
    let mut secret = [0u8; totp::SECRET_BYTES];
    env.fill(&mut secret);
    let mut nonce = [0u8; 24];
    env.fill(&mut nonce);
    let sealed = env.seal(&secret, identity.id().to_string().as_bytes(), nonce);
    identity.start_totp(sealed)?;
    identities.update(&mut identity).await?;
    Ok(TotpSetup {
        secret: totp::base32(&secret),
        uri: totp::otpauth_uri(issuer, account, &secret),
    })
}

/// Turns the second factor on once `code` from the app checks out. Returns the
/// recovery codes, which are never shown again.
pub async fn confirm_totp<R: IdentityRepo>(
    identities: &R,
    env: &impl AuthEnv,
    identity: IdentityId,
    code: &str,
) -> Result<Vec<String>, CommandError> {
    let mut identity = load_identity(identities, identity).await?;
    let (sealed, confirmed, last_step) =
        identity.totp().ok_or(IdentityError::MfaSetupNotStarted)?;
    if confirmed {
        return Err(IdentityError::MfaAlreadyEnabled.into());
    }
    let secret = env.open(sealed, identity.id().to_string().as_bytes())?;
    let step =
        totp::verify(&secret, code, env.now(), last_step).ok_or(CommandError::InvalidMfaCode)?;
    let codes = recovery::generate(env);
    identity.confirm_totp(step, codes.hashes)?;
    identities.update(&mut identity).await?;
    Ok(codes.codes)
}

/// Turns the second factor off. Needs a current TOTP or recovery code.
pub async fn disable_mfa<R: IdentityRepo>(
    identities: &R,
    env: &impl AuthEnv,
    identity: IdentityId,
    code: &str,
) -> Result<(), CommandError> {
    let mut identity = load_identity(identities, identity).await?;
    if !identity.mfa_enabled() {
        return Err(IdentityError::MfaNotEnabled.into());
    }
    if !use_second_factor(&mut identity, env, code)? {
        return Err(CommandError::InvalidMfaCode);
    }
    identity.disable_mfa()?;
    identities.update(&mut identity).await?;
    Ok(())
}

/// Replaces the recovery codes. Needs a current TOTP or recovery code.
pub async fn regenerate_recovery_codes<R: IdentityRepo>(
    identities: &R,
    env: &impl AuthEnv,
    identity: IdentityId,
    code: &str,
) -> Result<Vec<String>, CommandError> {
    let mut identity = load_identity(identities, identity).await?;
    if !identity.mfa_enabled() {
        return Err(IdentityError::MfaNotEnabled.into());
    }
    if !use_second_factor(&mut identity, env, code)? {
        return Err(CommandError::InvalidMfaCode);
    }
    let codes = recovery::generate(env);
    identity.set_recovery_codes(codes.hashes)?;
    identities.update(&mut identity).await?;
    Ok(codes.codes)
}

// ---------- API keys ----------

pub struct CreateApiKey {
    pub kind: ApiKeyKind,
    pub name: String,
    /// Required for a secret key; `None` for a publishable one.
    pub scopes: Option<Vec<Scope>>,
}

/// Creates a key and returns it with its token, which is never shown again. The caller
/// must hold every scope the key gets.
pub async fn create_api_key<R: ApiKeyRepo>(
    ctx: &Ctx,
    keys: &R,
    env: &(impl IdGenerator + Clock + Random),
    input: CreateApiKey,
) -> Result<(ApiKey, NewToken), CommandError> {
    ctx.require(Scope::ApiKeysWrite)?;
    let token = NewToken::generate(
        match input.kind {
            ApiKeyKind::Secret => TokenKind::SecretKey,
            ApiKeyKind::Publishable => TokenKind::PublishableKey,
        },
        env,
    );
    let parts = ApiKeyParts {
        id: ApiKeyId::from_uuid(env.new_uuid()),
        kind: input.kind,
        name: input.name,
        scopes: input.scopes,
        created_by: ctx.actor(),
        created_at: env.now(),
    };
    let mut key = ApiKey::new(parts, token.hash(), token.last_four())?;
    require_grantable(ctx, key.scopes())?;
    keys.insert(&mut key).await?;
    Ok((key, token))
}

pub async fn get_api_key<R: ApiKeyRepo>(
    ctx: &Ctx,
    keys: &R,
    id: ApiKeyId,
) -> Result<ApiKey, CommandError> {
    ctx.require(Scope::ApiKeysRead)?;
    keys.get(id).await?.ok_or(CommandError::ApiKeyNotFound(id))
}

/// Stops a key working, from the next request on.
pub async fn revoke_api_key<R: ApiKeyRepo>(
    ctx: &Ctx,
    keys: &R,
    clock: &impl Clock,
    id: ApiKeyId,
) -> Result<ApiKey, CommandError> {
    ctx.require(Scope::ApiKeysWrite)?;
    let mut key = keys
        .get(id)
        .await?
        .ok_or(CommandError::ApiKeyNotFound(id))?;
    if !key.revoke(clock.now()) {
        return Err(CommandError::ApiKeyRevoked(id));
    }
    keys.update(&mut key).await?;
    Ok(key)
}

/// The `Ctx` for a request carrying an API key: the key as the actor, with its scopes.
pub async fn authenticate_api_key<R: ApiKeyRepo>(
    keys: &R,
    token: &str,
) -> Result<Ctx, CommandError> {
    if !matches!(
        TokenKind::of(token),
        Some(TokenKind::SecretKey | TokenKind::PublishableKey)
    ) {
        return Err(CommandError::InvalidToken);
    }
    let key = keys
        .get_by_token(TokenHash::of(token))
        .await?
        .filter(|k| k.revoked_at().is_none())
        .ok_or(CommandError::InvalidToken)?;
    Ok(Ctx::new(Actor::ApiKey(key.id()), key.scopes().to_vec()))
}
