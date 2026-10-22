//! Authentication building blocks. `hypha-users` composes them into the staff flows (login,
//! invites); hosts call the API key ones directly.
//!
//! The commands acting on "my own" identity (`start_totp`, `change_password`, …) take
//! the identity or session the host resolved from the request's token, never one from
//! the request body: that is their authorization.

use hypha_core::{
    Actor, ApiKeyId, AuthError, ChannelId, Clock, Ctx, Email, ErrorCode, HasBatch, IdGenerator,
    IdentityId, PasswordResetId, Random, RepoError, Scope, SessionId, Timestamp,
};
use thiserror::Error;

use crate::{
    ApiKey, ApiKeyError, Identity, IdentityError, PasswordReset, Session,
    api_key::{ApiKeyKind, ApiKeyParts},
    identity::{CredentialKind, PrincipalKind},
    password::{HashError, Password, PasswordError, PasswordHasher},
    recovery,
    repo::{
        ApiKeyRepo, HasApiKeys, HasIdentities, HasPasswordResets, HasSessions, IdentityRepo,
        PasswordResetRepo, SessionRepo, StageIdentities, StagePasswordResets, StageSessions,
    },
    secret_box::{SealError, SecretBox},
    settings::HasAuthSettings,
    token::{NewToken, TokenHash, TokenKind},
    totp,
};

/// The non-storage ports auth commands use, as one bound. Like `hypha_cart::CartPorts`, the
/// blanket impl means nobody implements it by hand.
pub trait AuthEnv:
    IdGenerator + Clock + Random + PasswordHasher + SecretBox + HasAuthSettings
{
}

impl<T: IdGenerator + Clock + Random + PasswordHasher + SecretBox + HasAuthSettings> AuthEnv for T {}

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
    /// Too many wrong second-factor codes; no code is checked until `until`.
    #[error("too many wrong codes; try again after {until:?}")]
    MfaLocked { until: Timestamp },
    #[error("the current password is wrong")]
    WrongPassword,
    #[error("api key {0} not found")]
    ApiKeyNotFound(ApiKeyId),
    #[error("api key {0} is already revoked")]
    ApiKeyRevoked(ApiKeyId),
    #[error("identity {0} not found")]
    IdentityNotFound(IdentityId),
    /// Granting a scope the caller doesn't hold.
    #[error("you can't grant {0}, which you don't have")]
    CannotGrant(Scope),
    #[error("channel {0} not found")]
    ChannelNotFound(ChannelId),
    /// Not one of the caller's live sessions (someone else's reads the same).
    #[error("session {0} not found")]
    SessionNotFound(SessionId),
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
            CommandError::MfaLocked { .. } => "mfa_locked",
            CommandError::WrongPassword => "wrong_password",
            CommandError::ApiKeyNotFound(_) => "api_key_not_found",
            CommandError::ApiKeyRevoked(_) => "api_key_revoked",
            CommandError::IdentityNotFound(_) => "identity_not_found",
            CommandError::CannotGrant(_) => "not_allowed_to_grant",
            CommandError::ChannelNotFound(_) => "channel_not_found",
            CommandError::SessionNotFound(_) => "session_not_found",
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

/// Where a login stands after the password.
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

/// The session for an identity whose first factor checked out, not yet stored: active,
/// or pending if it has a second factor. For callers that store it in a batch with
/// other writes (`hypha_users::login`); `open_session` stores it alone.
pub fn prepare_login(identity: &Identity, env: &impl AuthEnv) -> LoginStep {
    let id = SessionId::from_uuid(env.new_uuid());
    let token = NewToken::generate(TokenKind::Session, env);
    let now = env.now();
    if identity.mfa_enabled() {
        let session = Session::pending(
            id,
            identity.id(),
            identity.principal(),
            token.hash(),
            now,
            env.auth_settings().second_factor_window,
        );
        let mut methods = vec![MfaMethod::Totp];
        if identity.recovery_codes_left() > 0 {
            methods.push(MfaMethod::RecoveryCode);
        }
        LoginStep::MfaRequired {
            session,
            token,
            methods,
        }
    } else {
        let session = Session::active(
            id,
            identity.id(),
            identity.principal(),
            token.hash(),
            now,
            None,
            env.auth_settings().session_lifetime,
        );
        LoginStep::Authenticated { session, token }
    }
}

impl LoginStep {
    /// The session either way, for storing it.
    pub fn session_mut(&mut self) -> &mut Session {
        match self {
            LoginStep::Authenticated { session, .. } | LoginStep::MfaRequired { session, .. } => {
                session
            }
        }
    }
}

/// Opens (stores) a session for an identity whose first factor checked out; see
/// `prepare_login`.
pub async fn open_session<R: SessionRepo>(
    sessions: &R,
    env: &impl AuthEnv,
    identity: &Identity,
) -> Result<LoginStep, CommandError> {
    let mut step = prepare_login(identity, env);
    sessions.insert(step.session_mut()).await?;
    Ok(step)
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
        env.auth_settings().session_lifetime,
    );
    (session, token)
}

/// How often a second-factor check reloads and tries again when a concurrent request
/// changed the identity or session first. Each round commits before answering, so a
/// round that runs out reports `concurrent_modification` without revealing the code's
/// result.
const MAX_ROUNDS: usize = 10;

/// Checks a second-factor `code` against `identity` and uses it up: a 6-digit TOTP
/// code (its step is recorded) or a recovery code (removed). A wrong code is counted on
/// the identity (`record_mfa_failure`), and while checks are locked no code is even
/// looked at. The caller must commit `identity` before answering either way: that is
/// what makes every guess count, even many sent at once (each would otherwise read the
/// same count).
fn use_second_factor(
    identity: &mut Identity,
    env: &impl AuthEnv,
    code: &str,
) -> Result<Factor, CommandError> {
    let now = env.now();
    if let Some(until) = identity.mfa_locked_until(now) {
        return Err(CommandError::MfaLocked { until });
    }
    let code = code.trim();
    let check = if code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()) {
        match identity.totp() {
            Some((sealed, true, last_step)) => {
                let secret = env.open(sealed, identity.id().to_string().as_bytes())?;
                totp::check(&secret, code, now, last_step)
            }
            _ => totp::Check::Wrong,
        }
    } else if recovery::hash(code).is_some_and(|hash| identity.use_recovery_code(hash)) {
        return Ok(Factor::Right);
    } else {
        totp::Check::Wrong
    };
    match check {
        totp::Check::Valid(step) => {
            identity.record_totp_step(step)?;
            Ok(Factor::Right)
        }
        // The right code, already used (a double submit, often): refused, but not a
        // guess, so it doesn't count toward the lockout.
        totp::Check::Replayed => Ok(Factor::Replayed),
        totp::Check::Wrong => {
            identity.record_mfa_failure(now, env.auth_settings().second_factor_lock);
            Ok(Factor::Wrong)
        }
    }
}

/// What `use_second_factor` made of a code. Only `Right` and `Wrong` change the
/// identity (and need writing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Factor {
    Right,
    Wrong,
    Replayed,
}

/// Finishes a login that owes a second factor: `challenge` is the pending session's
/// token, `code` a TOTP or recovery code. Returns the session under a new token.
///
/// A wrong code counts twice: on the session (the fifth ends it) and on the identity
/// (the fifth locks checks for a while, across every challenge). Both counts are
/// committed before the answer; when a concurrent request changed either first, the
/// round starts again on fresh data, so guesses sent in parallel can't share a count.
pub async fn finish_login<S>(
    ports: &S,
    env: &impl AuthEnv,
    challenge: &str,
    code: &str,
) -> Result<(Session, NewToken), CommandError>
where
    S: HasIdentities + HasSessions + HasBatch<Batch: StageIdentities + StageSessions>,
{
    if TokenKind::of(challenge) != Some(TokenKind::Session) {
        return Err(CommandError::InvalidToken);
    }
    for _ in 0..MAX_ROUNDS {
        let now = env.now();
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

        let factor = use_second_factor(&mut identity, env, code)?;
        let right = factor == Factor::Right;
        let token = NewToken::generate(TokenKind::Session, env);
        if right {
            session.activate(token.hash(), now, env.auth_settings().session_lifetime);
        } else {
            // A replayed code counts on the challenge (it's no way in), not the identity.
            session.record_failed_attempt(now);
        }
        let mut batch = ports.batch();
        if factor != Factor::Replayed {
            batch.update_identity(&mut identity);
        }
        batch.update_session(&mut session);
        match ports.commit(batch).await {
            Ok(()) if right => return Ok((session, token)),
            Ok(()) => return Err(CommandError::InvalidMfaCode),
            Err(RepoError::StaleVersion) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(RepoError::StaleVersion.into())
}

/// Checks `code` on `identity`'s second factor and, if it is right, applies `change`;
/// then commits, wrong code or not (see `use_second_factor`), retrying on concurrent
/// changes like `finish_login`.
async fn with_second_factor<R: IdentityRepo, T>(
    identities: &R,
    env: &impl AuthEnv,
    id: IdentityId,
    code: &str,
    mut change: impl FnMut(&mut Identity) -> Result<T, CommandError>,
) -> Result<T, CommandError> {
    for _ in 0..MAX_ROUNDS {
        let mut identity = load_identity(identities, id).await?;
        if !identity.mfa_enabled() {
            return Err(IdentityError::MfaNotEnabled.into());
        }
        let result = match use_second_factor(&mut identity, env, code)? {
            Factor::Right => Some(change(&mut identity)?),
            Factor::Wrong => None,
            // Nothing changed, so nothing to write.
            Factor::Replayed => return Err(CommandError::InvalidMfaCode),
        };
        match identities.update(&mut identity).await {
            Ok(()) => return result.ok_or(CommandError::InvalidMfaCode),
            Err(RepoError::StaleVersion) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(RepoError::StaleVersion.into())
}

/// The hash a session `token` is stored under, or `InvalidToken` if it isn't a session
/// token: checked before any lookup.
pub fn session_token_hash(token: &str) -> Result<TokenHash, CommandError> {
    if TokenKind::of(token) != Some(TokenKind::Session) {
        return Err(CommandError::InvalidToken);
    }
    Ok(TokenHash::of(token))
}

/// The active session behind `token`, or `InvalidToken`.
pub async fn authenticate_session<R: SessionRepo>(
    sessions: &R,
    clock: &impl Clock,
    token: &str,
) -> Result<Session, CommandError> {
    sessions
        .get_by_token(session_token_hash(token)?)
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

// ---------- a staff member's own sessions ----------

/// The logged-in sessions of `current`'s identity (its own included), newest first.
/// Pending logins aren't listed: they can't do anything but finish.
pub async fn list_sessions<R: SessionRepo>(
    sessions: &R,
    clock: &impl Clock,
    current: &Session,
) -> Result<Vec<Session>, CommandError> {
    let now = clock.now();
    let mut live = sessions.live_for_identity(current.identity(), now).await?;
    live.retain(|s| s.is_active(now));
    Ok(live)
}

/// Ends session `id`, one of `current`'s identity's (`current` itself is a logout). Any
/// other session, ended or not, is `SessionNotFound`: nobody learns whether another
/// account's session exists.
pub async fn revoke_session<R: SessionRepo>(
    sessions: &R,
    clock: &impl Clock,
    current: &Session,
    id: SessionId,
) -> Result<(), CommandError> {
    let now = clock.now();
    let mut session = sessions
        .get(id)
        .await?
        .filter(|s| s.identity() == current.identity() && s.is_live(now))
        .ok_or(CommandError::SessionNotFound(id))?;
    session.revoke(now);
    sessions.update(&mut session).await?;
    Ok(())
}

/// Ends every session of `current`'s identity but `current` ("sign out everywhere
/// else").
pub async fn revoke_other_sessions<S>(
    ports: &S,
    clock: &impl Clock,
    current: &Session,
) -> Result<(), CommandError>
where
    S: HasBatch<Batch: StageSessions>,
{
    let mut batch = ports.batch();
    batch.revoke_sessions(current.identity(), Some(current.id()), clock.now());
    ports.commit(batch).await?;
    Ok(())
}

/// Records that a request used `session`, at most once per `Session::USE_RESOLUTION`,
/// so listing sessions can say when each was last active without a write per request.
pub async fn record_session_use<R: SessionRepo>(
    sessions: &R,
    clock: &impl Clock,
    session: &mut Session,
) -> Result<(), CommandError> {
    let now = clock.now();
    if session.use_is_stale(now) {
        sessions.record_use(session.id(), now).await?;
        session.record_use(now);
    }
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

/// `WrongPassword` unless `password` is the identity's current one.
async fn verify_current_password(
    env: &impl AuthEnv,
    identity: &Identity,
    password: &str,
) -> Result<(), CommandError> {
    let hash = identity
        .password()
        .map(|(_, hash)| hash.clone())
        .ok_or(IdentityError::NoPassword)?;
    let password = Password::attempt(password).ok_or(CommandError::WrongPassword)?;
    if env.verify_password(&password, &hash).await? {
        Ok(())
    } else {
        Err(CommandError::WrongPassword)
    }
}

/// Changes the password of `session`'s identity, logs out its other sessions and uses
/// up any open reset links.
pub async fn change_password<S>(
    ports: &S,
    env: &impl AuthEnv,
    session: &Session,
    current: &str,
    new: &str,
) -> Result<(), CommandError>
where
    S: HasIdentities + HasBatch<Batch: StageIdentities + StageSessions + StagePasswordResets>,
{
    let new = Password::new(new)?;
    let mut identity = load_identity(ports.identities(), session.identity()).await?;
    verify_current_password(env, &identity, current).await?;
    identity.set_password_hash(hash_new_password(env, &new).await?)?;
    let mut batch = ports.batch();
    batch.update_identity(&mut identity);
    batch.revoke_sessions(identity.id(), Some(session.id()), env.now());
    // A reset link sent before would otherwise undo the change.
    batch.use_up_password_resets(identity.id(), env.now());
    ports.commit(batch).await?;
    Ok(())
}

/// A reset for the identity of `kind` that logs in with `email`, or `None` if there is
/// none, or if its latest link still works and is younger than
/// `AuthSettings::password_reset_resend_after` (`PasswordReset::blocks_another`). The caller answers the same either way, and
/// emails the token when there is one.
pub async fn start_password_reset<S>(
    ports: &S,
    env: &impl AuthEnv,
    kind: PrincipalKind,
    email: &str,
) -> Result<Option<(Identity, PasswordReset, NewToken)>, CommandError>
where
    S: HasIdentities + HasPasswordResets + HasBatch<Batch: StageIdentities + StagePasswordResets>,
{
    let Ok(email) = Email::new(email) else {
        return Ok(None);
    };
    let Some(mut identity) = ports
        .identities()
        .find_login(kind, CredentialKind::Password, email.as_str())
        .await?
    else {
        return Ok(None);
    };
    // Asked again while the link already sent works and is recent: it stands (no email
    // bomb).
    let latest = ports
        .password_resets()
        .latest_for_identity(identity.id())
        .await?;
    if latest.is_some_and(|r| {
        r.blocks_another(env.now(), env.auth_settings().password_reset_resend_after)
    }) {
        return Ok(None);
    }
    let token = NewToken::generate(TokenKind::PasswordReset, env);
    let mut reset = PasswordReset::new(
        PasswordResetId::from_uuid(env.new_uuid()),
        identity.id(),
        token.hash(),
        env.now(),
        env.auth_settings().password_reset_lifetime,
    );
    // The identity is written too (unchanged, at its next version): two requests that
    // both passed the check above then conflict on it, and only one link is made.
    let mut batch = ports.batch();
    batch.update_identity(&mut identity);
    batch.insert_password_reset(&mut reset);
    match ports.commit(batch).await {
        Ok(()) => Ok(Some((identity, reset, token))),
        // A concurrent request made the link (or the identity changed meanwhile, e.g. a
        // code was used): either way, no second email now.
        Err(RepoError::StaleVersion) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Uses up `identity`'s open reset links, e.g. one whose email couldn't be sent, so it
/// doesn't hold back the next request (`PasswordReset::blocks_another`).
pub async fn withdraw_password_resets<S>(
    ports: &S,
    clock: &impl Clock,
    identity: IdentityId,
) -> Result<(), CommandError>
where
    S: HasBatch<Batch: StagePasswordResets>,
{
    let mut batch = ports.batch();
    batch.use_up_password_resets(identity, clock.now());
    Ok(ports.commit(batch).await?)
}

/// Sets a new password with an emailed reset token, uses up that token and every other
/// open one of the identity, and logs out all its sessions, in one batch.
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
    // Any other link that went out (a second request, a leaked email) stops working too.
    batch.use_up_password_resets(identity.id(), now);
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
///
/// Needs the current `password`: otherwise a stolen session could put the thief's
/// authenticator on the account and lock its owner out (a password reset keeps 2FA).
pub async fn start_totp<R: IdentityRepo>(
    identities: &R,
    env: &impl AuthEnv,
    identity: IdentityId,
    password: &str,
    issuer: &str,
    account: &str,
) -> Result<TotpSetup, CommandError> {
    let mut identity = load_identity(identities, identity).await?;
    verify_current_password(env, &identity, password).await?;
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
/// recovery codes, which are never shown again. Wrong codes count toward the lockout
/// like everywhere else, committed before the answer (see `use_second_factor`):
/// otherwise a stolen session could guess its way into confirming a pending setup and
/// walk away with the recovery codes.
pub async fn confirm_totp<R: IdentityRepo>(
    identities: &R,
    env: &impl AuthEnv,
    identity: IdentityId,
    code: &str,
) -> Result<Vec<String>, CommandError> {
    for _ in 0..MAX_ROUNDS {
        let now = env.now();
        let mut identity = load_identity(identities, identity).await?;
        let (sealed, confirmed, last_step) =
            identity.totp().ok_or(IdentityError::MfaSetupNotStarted)?;
        if confirmed {
            return Err(IdentityError::MfaAlreadyEnabled.into());
        }
        if let Some(until) = identity.mfa_locked_until(now) {
            return Err(CommandError::MfaLocked { until });
        }
        let secret = env.open(sealed, identity.id().to_string().as_bytes())?;
        let codes = match totp::check(&secret, code, now, last_step) {
            totp::Check::Valid(step) => {
                let codes = recovery::generate(env);
                identity.confirm_totp(step, codes.hashes)?;
                Some(codes.codes)
            }
            // Nothing changed, so nothing to write.
            totp::Check::Replayed => return Err(CommandError::InvalidMfaCode),
            totp::Check::Wrong => {
                identity.record_mfa_failure(now, env.auth_settings().second_factor_lock);
                None
            }
        };
        match identities.update(&mut identity).await {
            Ok(()) => return codes.ok_or(CommandError::InvalidMfaCode),
            Err(RepoError::StaleVersion) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(RepoError::StaleVersion.into())
}

/// Turns the second factor off. Needs a current TOTP or recovery code; wrong ones
/// count toward the lockout like at login.
pub async fn disable_mfa<R: IdentityRepo>(
    identities: &R,
    env: &impl AuthEnv,
    identity: IdentityId,
    code: &str,
) -> Result<(), CommandError> {
    with_second_factor(identities, env, identity, code, |i| Ok(i.disable_mfa()?)).await
}

/// Replaces the recovery codes. Needs a current TOTP or recovery code; wrong ones
/// count toward the lockout like at login.
pub async fn regenerate_recovery_codes<R: IdentityRepo>(
    identities: &R,
    env: &impl AuthEnv,
    identity: IdentityId,
    code: &str,
) -> Result<Vec<String>, CommandError> {
    with_second_factor(identities, env, identity, code, |i| {
        let codes = recovery::generate(env);
        i.set_recovery_codes(codes.hashes)?;
        Ok(codes.codes)
    })
    .await
}

// ---------- API keys ----------

pub struct CreateApiKey {
    pub kind: ApiKeyKind,
    pub name: String,
    /// Required for a secret key; `None` for a publishable one.
    pub scopes: Option<Vec<Scope>>,
    /// Required for a publishable key (the channel its storefront sells through);
    /// `None` for a secret one.
    pub channel: Option<ChannelId>,
}

/// Whether a sales channel exists: what creating a publishable key asks, without
/// `hypha-auth` depending on `hypha-channels`. Storage adapters, which see both, implement
/// it.
#[allow(async_fn_in_trait)]
pub trait ChannelDirectory {
    async fn channel_exists(&self, channel: ChannelId) -> Result<bool, RepoError>;
}

/// Creates a key and returns it with its token, which is never shown again. The caller
/// must hold every scope the key gets. The key's own rules (scopes and channel by kind)
/// are checked before its channel is looked up, so a secret key sent with a channel is
/// told it can't have one, whatever that channel is.
pub async fn create_api_key<S: HasApiKeys + ChannelDirectory>(
    ctx: &Ctx,
    ports: &S,
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
        channel: input.channel,
        created_by: ctx.actor(),
        created_at: env.now(),
    };
    let mut key = ApiKey::new(parts, token.hash(), token.last_four())?;
    require_grantable(ctx, key.scopes())?;
    if let Some(channel) = key.channel()
        && !ports.channel_exists(channel).await?
    {
        return Err(CommandError::ChannelNotFound(channel));
    }
    ports.api_keys().insert(&mut key).await?;
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

/// A page of the store's API keys, revoked ones too, newest first.
pub async fn list_api_keys<R: ApiKeyRepo>(
    ctx: &Ctx,
    keys: &R,
    page: hypha_core::PageRequest<ApiKeyId>,
) -> Result<hypha_core::Page<ApiKey>, CommandError> {
    ctx.require(Scope::ApiKeysRead)?;
    Ok(keys.list(page).await?)
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

/// The `Ctx` for a request carrying an API key: the key as the actor, with its scopes, and
/// a publishable key's channel.
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
    let actor = Actor::ApiKey(key.id());
    Ok(match key.channel() {
        Some(channel) => Ctx::in_channel(actor, key.scopes().to_vec(), channel),
        None => Ctx::new(actor, key.scopes().to_vec()),
    })
}
