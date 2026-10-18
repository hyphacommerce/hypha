//! Staff flows (design note 0008): the owner's first start, roles, invites, login and
//! password reset, composing `hypha-auth` with users and roles.

use hypha_auth::{
    AuthEnv, HasIdentities, HasPasswordResets, HasSessions, Identity, IdentityRepo, LoginStep,
    NewToken, Password, Principal, PrincipalKind, Session, SessionClient, StageIdentities,
    StagePasswordResets, StageSessions, TokenHash, TokenKind, hash_new_password,
    new_active_session, record_session_use, require_grantable,
};
use hypha_core::{
    Actor, AuthError, Clock, Ctx, Email, EmailError, ErrorCode, HasBatch, IdGenerator, IdentityId,
    InviteId, Notifier, NotifyError, RepoError, RoleId, Scope, Timestamp, UserId, report_duplicate,
};
use thiserror::Error;

use crate::{
    Invite, Role, RoleError, User, UserError, UserStatus,
    repo::{
        HasInvites, HasRoles, HasStaffSessions, HasUsers, InviteRepo, RoleRepo, StaffSessionRecord,
        StaffSessionRepo, StageInvites, StageRoles, StageUsers, UserRepo, keys,
    },
};

/// Every port the staff flows use, as one bound. The blanket impl means hosts and tests
/// never implement it by hand (like `hypha_cart::CartPorts`).
pub trait StaffPorts:
    HasUsers
    + HasRoles
    + HasInvites
    + HasIdentities
    + HasSessions
    + HasPasswordResets
    + HasBatch<
        Batch: StageUsers
                   + StageRoles
                   + StageInvites
                   + StageIdentities
                   + StageSessions
                   + StagePasswordResets,
    >
{
}

impl<T> StaffPorts for T where
    T: HasUsers
        + HasRoles
        + HasInvites
        + HasIdentities
        + HasSessions
        + HasPasswordResets
        + HasBatch<
            Batch: StageUsers
                       + StageRoles
                       + StageInvites
                       + StageIdentities
                       + StageSessions
                       + StagePasswordResets,
        >
{
}

/// What the staff flows tell people, for the host to render and deliver (design note
/// 0008, section 7). Tokens are secrets: the message is their only copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaffEmail {
    Invite {
        to: Email,
        token: String,
        expires_at: Timestamp,
    },
    PasswordReset {
        to: Email,
        token: String,
        expires_at: Timestamp,
    },
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Login(#[from] hypha_auth::CommandError),
    #[error(transparent)]
    Role(#[from] RoleError),
    #[error(transparent)]
    User(#[from] UserError),
    #[error(transparent)]
    Email(#[from] EmailError),
    #[error(transparent)]
    Password(#[from] hypha_auth::PasswordError),
    /// The write succeeded but the email didn't go out; doing it again resends.
    #[error(transparent)]
    Notify(#[from] NotifyError),
    #[error("role {0} not found")]
    RoleNotFound(RoleId),
    #[error("a role named {0:?} already exists")]
    RoleNameTaken(String),
    #[error("user {0} not found")]
    UserNotFound(UserId),
    #[error("{0} is already a staff member")]
    UserExists(Email),
    /// The last active owner can't be disabled or given another role.
    #[error("the store needs at least one active owner")]
    LastOwner,
    #[error("the session belongs to no staff member")]
    NotStaff,
    #[error("invite {0} not found")]
    InviteNotFound(InviteId),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Login(e) => e.code(),
            CommandError::Role(e) => e.code(),
            CommandError::User(e) => e.code(),
            CommandError::Email(e) => e.code(),
            CommandError::Password(e) => e.code(),
            CommandError::Notify(e) => e.code(),
            CommandError::RoleNotFound(_) => "role_not_found",
            CommandError::RoleNameTaken(_) => "role_name_taken",
            CommandError::UserNotFound(_) => "user_not_found",
            CommandError::UserExists(_) => "user_exists",
            CommandError::LastOwner => "last_owner",
            CommandError::NotStaff => "invalid_token",
            CommandError::InviteNotFound(_) => "invite_not_found",
        }
    }
}

impl From<hypha_auth::IdentityError> for CommandError {
    fn from(e: hypha_auth::IdentityError) -> Self {
        CommandError::Login(e.into())
    }
}

// ---------- first start ----------

pub struct NewOwner {
    pub email: String,
    pub name: String,
    pub password: String,
}

/// With no staff user yet, creates the owner role (if missing) and the owner, who can
/// log in with `password` (design note 0008, A11). `None` once anyone exists: the
/// environment variables that feed this are ignored from then on.
pub async fn bootstrap_owner<S: StaffPorts>(
    ctx: &Ctx,
    ports: &S,
    env: &impl AuthEnv,
    input: NewOwner,
) -> Result<Option<User>, CommandError> {
    ctx.require(Scope::UsersWrite)?;
    if ports.users().any().await? {
        return Ok(None);
    }
    let email = Email::new(&input.email)?;
    let password = Password::new(&input.password)?;
    let mut batch = ports.batch();
    let mut new_role = None;
    let role = match ports.roles().owner().await? {
        Some(role) => role,
        None => new_role
            .insert(Role::owner(RoleId::from_uuid(env.new_uuid())))
            .clone(),
    };
    let mut user = User::new(
        UserId::from_uuid(env.new_uuid()),
        email.clone(),
        &input.name,
        role.id(),
        env.now(),
    )?;
    let mut identity = Identity::with_password(
        IdentityId::from_uuid(env.new_uuid()),
        Principal::Staff(user.id()),
        email,
        hash_new_password(env, &password).await?,
    );
    if let Some(role) = &mut new_role {
        batch.insert_role(role);
    }
    batch.insert_user(&mut user);
    batch.insert_identity(&mut identity);
    match ports.commit(batch).await {
        Ok(()) => Ok(Some(user)),
        // Another instance starting at the same time made the owner first (a rolling
        // deploy): the same answer as finding them already there.
        Err(RepoError::Duplicate(_)) if ports.users().any().await? => Ok(None),
        Err(e) => Err(e.into()),
    }
}

// ---------- roles ----------

pub async fn create_role<R: RoleRepo>(
    ctx: &Ctx,
    roles: &R,
    ids: &impl IdGenerator,
    name: &str,
    scopes: &[Scope],
) -> Result<Role, CommandError> {
    ctx.require(Scope::UsersWrite)?;
    let mut role = Role::new(RoleId::from_uuid(ids.new_uuid()), name, scopes)?;
    require_grantable(ctx, &role.granted())?;
    if let Some(taken) = name_taken(roles, &role).await? {
        return Err(taken);
    }
    let written = roles.insert(&mut role).await;
    report_duplicate(written, keys::ROLE_NAME, name_taken(roles, &role)).await?;
    Ok(role)
}

async fn name_taken<R: RoleRepo>(
    roles: &R,
    role: &Role,
) -> Result<Option<CommandError>, CommandError> {
    Ok(roles
        .get_by_name(role.name())
        .await?
        .filter(|other| other.id() != role.id())
        .map(|other| CommandError::RoleNameTaken(other.name().to_string())))
}

async fn load_role<R: RoleRepo>(roles: &R, id: RoleId) -> Result<Role, CommandError> {
    roles.get(id).await?.ok_or(CommandError::RoleNotFound(id))
}

/// Every role, oldest first (design note 0024, AU6).
pub async fn list_roles<R: RoleRepo>(ctx: &Ctx, roles: &R) -> Result<Vec<Role>, CommandError> {
    ctx.require(Scope::UsersRead)?;
    Ok(roles.list_all().await?)
}

/// Every staff user, oldest first (design note 0024, AU6).
pub async fn list_users<R: UserRepo>(ctx: &Ctx, users: &R) -> Result<Vec<User>, CommandError> {
    ctx.require(Scope::UsersRead)?;
    Ok(users.list_all().await?)
}

/// Every invite not yet accepted, oldest first; expired ones too, so staff can renew
/// them (inviting the same email again).
pub async fn list_invites<R: InviteRepo>(
    ctx: &Ctx,
    invites: &R,
) -> Result<Vec<Invite>, CommandError> {
    ctx.require(Scope::UsersRead)?;
    Ok(invites.pending().await?)
}

pub async fn get_role<R: RoleRepo>(ctx: &Ctx, roles: &R, id: RoleId) -> Result<Role, CommandError> {
    ctx.require(Scope::UsersRead)?;
    load_role(roles, id).await
}

/// Fields left `None` stay as they are.
#[derive(Default)]
pub struct UpdateRole {
    pub name: Option<String>,
    pub scopes: Option<Vec<Scope>>,
}

/// The caller must hold every scope the role grants, before and after: nobody edits a
/// role more powerful than themselves.
pub async fn update_role<R: RoleRepo>(
    ctx: &Ctx,
    roles: &R,
    id: RoleId,
    input: UpdateRole,
) -> Result<Role, CommandError> {
    ctx.require(Scope::UsersWrite)?;
    let mut role = load_role(roles, id).await?;
    require_grantable(ctx, &role.granted())?;
    if let Some(name) = &input.name {
        role.rename(name)?;
    }
    if let Some(scopes) = &input.scopes {
        role.set_scopes(scopes)?;
        require_grantable(ctx, &role.granted())?;
    }
    if let Some(taken) = name_taken(roles, &role).await? {
        return Err(taken);
    }
    let written = roles.update(&mut role).await;
    report_duplicate(written, keys::ROLE_NAME, name_taken(roles, &role)).await?;
    Ok(role)
}

// ---------- users ----------

async fn load_user<R: UserRepo>(users: &R, id: UserId) -> Result<User, CommandError> {
    users.get(id).await?.ok_or(CommandError::UserNotFound(id))
}

pub async fn get_user<R: UserRepo>(ctx: &Ctx, users: &R, id: UserId) -> Result<User, CommandError> {
    ctx.require(Scope::UsersRead)?;
    load_user(users, id).await
}

/// Fields left `None` stay as they are.
#[derive(Default)]
pub struct UpdateUser {
    pub name: Option<String>,
    pub role: Option<RoleId>,
    pub status: Option<UserStatus>,
}

/// Changing someone's role or status needs every scope of their current role (and of
/// the new one), so nobody demotes or disables someone more powerful.
///
/// Disabling a user also revokes all their sessions and uses up their reset links, in
/// the same batch: otherwise enabling them again would bring back every token, a stolen
/// one included.
///
/// The last active owner keeps the owner role and stays active. Counting the owners and
/// then writing one user isn't enough on its own: two requests removing two different
/// owners would both count two. So a change that removes an active owner also writes
/// the owner role (unchanged, at its next version); two such changes at once then
/// conflict on it, and the later one fails with `concurrent_modification`.
pub async fn update_user<S: StaffPorts>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    id: UserId,
    input: UpdateUser,
) -> Result<User, CommandError> {
    ctx.require(Scope::UsersWrite)?;
    let mut user = load_user(ports.users(), id).await?;
    // Loaded before the owners are counted: if another request removes an owner after
    // this read, the role's version has moved and this batch fails.
    let mut current = load_role(ports.roles(), user.role()).await?;
    let was_active = user.is_active();
    let was_active_owner = current.is_owner() && was_active;
    let changes_role = input.role.is_some_and(|r| r != user.role());
    if changes_role || input.status.is_some_and(|s| s != user.status()) {
        require_grantable(ctx, &current.granted())?;
    }
    if let Some(name) = &input.name {
        user.rename(name)?;
    }
    if let Some(role) = input.role {
        let new = load_role(ports.roles(), role).await?;
        require_grantable(ctx, &new.granted())?;
        user.set_role(role);
    }
    if let Some(status) = input.status {
        user.set_status(status);
    }
    let removes_owner = was_active_owner && !(user.role() == current.id() && user.is_active());
    if removes_owner && ports.users().count_active_with_role(current.id()).await? <= 1 {
        return Err(CommandError::LastOwner);
    }

    let mut batch = ports.batch();
    batch.update_user(&mut user);
    if removes_owner {
        batch.update_role(&mut current);
    }
    if was_active
        && !user.is_active()
        && let Some(identity) = ports
            .identities()
            .get_by_principal(Principal::Staff(user.id()))
            .await?
    {
        batch.revoke_sessions(identity.id(), None, clock.now());
        // A reset link already sent would let whoever holds it set the password, and
        // take the account back when it's enabled again.
        batch.use_up_password_resets(identity.id(), clock.now());
    }
    ports.commit(batch).await?;
    Ok(user)
}

/// Renames the staff member `me` (the caller's own user, from their session): no scope
/// needed, since everyone may name themselves. Retried when the user moved meanwhile (a
/// login writes the user too).
pub async fn rename_me<R: UserRepo>(
    users: &R,
    me: UserId,
    name: &str,
) -> Result<User, CommandError> {
    const ROUNDS: usize = 3;
    for _ in 0..ROUNDS {
        let mut user = load_user(users, me).await?;
        user.rename(name)?;
        match users.update(&mut user).await {
            Ok(()) => return Ok(user),
            Err(RepoError::StaleVersion) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(RepoError::StaleVersion.into())
}

// ---------- invites ----------

pub struct InviteUser {
    pub email: String,
    pub role: RoleId,
}

/// Invites `email` with `role`, or renews the pending invite for that email (new token,
/// new week, maybe a new role), then emails the token. A failed email leaves the
/// invite in place: inviting again sends a fresh one.
pub async fn invite_user<S: HasUsers + HasRoles + HasInvites>(
    ctx: &Ctx,
    ports: &S,
    env: &(impl AuthEnv + Notifier<StaffEmail>),
    input: InviteUser,
) -> Result<Invite, CommandError> {
    ctx.require(Scope::UsersWrite)?;
    let email = Email::new(&input.email)?;
    let role = load_role(ports.roles(), input.role).await?;
    require_grantable(ctx, &role.granted())?;
    if ports.users().get_by_email(&email).await?.is_some() {
        return Err(CommandError::UserExists(email));
    }
    let token = NewToken::generate(TokenKind::Invite, env);
    let invite = save_invite(ctx, ports.invites(), env, &email, role.id(), token.hash()).await?;
    env.notify(StaffEmail::Invite {
        to: email,
        token: token.expose().to_string(),
        expires_at: invite.expires_at(),
    })
    .await?;
    Ok(invite)
}

/// Writes the invite for `email`: renews the pending one or inserts a new one. When a
/// concurrent invite for the same email gets in between (a double click), the insert
/// fails on `INVITE_EMAIL` or the renewal on the version: look again and renew that
/// one, so the race answers like a normal re-invite.
async fn save_invite<R: InviteRepo>(
    ctx: &Ctx,
    invites: &R,
    env: &impl AuthEnv,
    email: &Email,
    role: RoleId,
    token: TokenHash,
) -> Result<Invite, CommandError> {
    const ROUNDS: usize = 3;
    for _ in 0..ROUNDS {
        let written = match invites.get_by_email(email).await? {
            // Accepted, but the user is gone: can't happen while users can't be deleted.
            Some(invite) if invite.accepted_at().is_some() => {
                return Err(CommandError::UserExists(email.clone()));
            }
            Some(mut invite) => {
                invite.renew(
                    token,
                    role,
                    ctx.actor(),
                    env.now(),
                    env.auth_settings().invite_lifetime,
                );
                invites.update(&mut invite).await.map(|()| invite)
            }
            None => {
                let mut invite = Invite::new(
                    InviteId::from_uuid(env.new_uuid()),
                    email.clone(),
                    role,
                    token,
                    ctx.actor(),
                    env.now(),
                    env.auth_settings().invite_lifetime,
                );
                invites.insert(&mut invite).await.map(|()| invite)
            }
        };
        match written {
            Ok(invite) => return Ok(invite),
            Err(RepoError::Duplicate(keys::INVITE_EMAIL) | RepoError::StaleVersion) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(RepoError::StaleVersion.into())
}

pub struct AcceptInvite {
    pub token: String,
    pub name: String,
    pub password: String,
    /// Where the request came from, for the session it opens.
    pub client: SessionClient,
}

/// What an invite's link shows before it is accepted (`lookup_invite`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteDetails {
    pub invite: Invite,
    /// The role it gives.
    pub role: Role,
    /// Who sent it, when a staff member did and they still exist.
    pub inviter: Option<User>,
}

/// The invite behind `token`, for the page its link opens: the email, the role, who
/// sent it and until when it works. Needs no credentials, the token being one, so it
/// answers `invalid_token` for anything not usable (unknown, expired, accepted or
/// revoked), like accepting would.
pub async fn lookup_invite<S: HasInvites + HasRoles + HasUsers>(
    ports: &S,
    clock: &impl Clock,
    token: &str,
) -> Result<InviteDetails, CommandError> {
    if TokenKind::of(token) != Some(TokenKind::Invite) {
        return Err(hypha_auth::CommandError::InvalidToken.into());
    }
    let invite = ports
        .invites()
        .get_by_token(TokenHash::of(token))
        .await?
        .filter(|i| i.is_usable(clock.now()))
        .ok_or(hypha_auth::CommandError::InvalidToken)?;
    let role = load_role(ports.roles(), invite.role()).await?;
    let inviter = match invite.invited_by() {
        Actor::User(id) => ports.users().get(id).await?,
        _ => None,
    };
    Ok(InviteDetails {
        invite,
        role,
        inviter,
    })
}

/// Withdraws a pending invite: its link stops working, and the email can be invited
/// afresh. Like inviting, it needs every scope the invite's role grants, so nobody
/// cancels an invite more powerful than themselves. An accepted invite is a user now
/// (disable them instead), so it reads as not found.
pub async fn revoke_invite<S: HasInvites + HasRoles>(
    ctx: &Ctx,
    ports: &S,
    id: InviteId,
) -> Result<(), CommandError> {
    ctx.require(Scope::UsersWrite)?;
    let invite = ports
        .invites()
        .get(id)
        .await?
        .filter(|i| i.accepted_at().is_none())
        .ok_or(CommandError::InviteNotFound(id))?;
    // A role that's gone grants nothing, so anyone with `users:write` may clean it up.
    if let Some(role) = ports.roles().get(invite.role()).await? {
        require_grantable(ctx, &role.granted())?;
    }
    // Accepted or renewed meanwhile is `StaleVersion` (`concurrent_modification`): a
    // renewal is someone's newer invite, not this one.
    ports.invites().delete(&invite).await?;
    Ok(())
}

/// Creates the invited user with their password and logs them in, all in one batch, so
/// a failure leaves no half-made account (Medusa's two-step registration can).
pub async fn accept_invite<S: StaffPorts>(
    ports: &S,
    env: &impl AuthEnv,
    input: AcceptInvite,
) -> Result<(User, Session, NewToken), CommandError> {
    let now = env.now();
    if TokenKind::of(&input.token) != Some(TokenKind::Invite) {
        return Err(hypha_auth::CommandError::InvalidToken.into());
    }
    let mut invite = ports
        .invites()
        .get_by_token(TokenHash::of(&input.token))
        .await?
        .filter(|i| i.is_usable(now))
        .ok_or(hypha_auth::CommandError::InvalidToken)?;
    let password = Password::new(&input.password)?;
    load_role(ports.roles(), invite.role()).await?;
    let mut user = User::new(
        UserId::from_uuid(env.new_uuid()),
        invite.email().clone(),
        &input.name,
        invite.role(),
        now,
    )?;
    if ports.users().get_by_email(user.email()).await?.is_some() {
        return Err(CommandError::UserExists(user.email().clone()));
    }
    let mut identity = Identity::with_password(
        IdentityId::from_uuid(env.new_uuid()),
        Principal::Staff(user.id()),
        user.email().clone(),
        hash_new_password(env, &password).await?,
    );
    let (mut session, token) = new_active_session(&identity, env);
    session.set_client(input.client);
    invite.accept(now);

    let mut batch = ports.batch();
    batch.update_invite(&mut invite);
    batch.insert_user(&mut user);
    batch.insert_identity(&mut identity);
    batch.insert_session(&mut session);
    match ports.commit(batch).await {
        Err(RepoError::Duplicate(keys::USER_EMAIL)) => {
            Err(CommandError::UserExists(user.email().clone()))
        }
        // Accepted twice at once: the invite's version moved, and the loser's token is
        // used up.
        Err(RepoError::StaleVersion) => Err(hypha_auth::CommandError::InvalidToken.into()),
        Err(e) => Err(e.into()),
        Ok(()) => Ok((user, session, token)),
    }
}

// ---------- login ----------

/// The first login step for staff: email and password. A disabled user gets the same
/// answer as a wrong password.
///
/// The session is stored in a batch that also writes the user (unchanged, at its next
/// version). Disabling writes the user too, so a login and a disable at the same moment
/// conflict and one starts over: either the disable lands first and the login sees a
/// disabled user, or the login lands first and the disable revokes its session. Without
/// this, a disable committing while the login stored its session could miss it (on
/// Postgres, even if the login checked the user again afterwards), and enabling the user
/// later would bring that session to life.
pub async fn login<S: StaffPorts>(
    ports: &S,
    env: &impl AuthEnv,
    email: &str,
    password: &str,
    client: &SessionClient,
) -> Result<LoginStep, CommandError> {
    let identity = hypha_auth::check_password(
        ports.identities(),
        env,
        PrincipalKind::Staff,
        email,
        password,
    )
    .await?;
    let Principal::Staff(user) = identity.principal();
    const ROUNDS: usize = 3;
    for _ in 0..ROUNDS {
        let Some(mut user) = ports.users().get(user).await?.filter(User::is_active) else {
            return Err(hypha_auth::CommandError::InvalidCredentials.into());
        };
        let mut step = hypha_auth::prepare_login(&identity, env);
        step.session_mut().set_client(client.clone());
        let mut batch = ports.batch();
        batch.update_user(&mut user);
        batch.insert_session(step.session_mut());
        match ports.commit(batch).await {
            Ok(()) => return Ok(step),
            // The user changed meanwhile (maybe disabled): look again.
            Err(RepoError::StaleVersion) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(RepoError::StaleVersion.into())
}

/// A request's staff caller: the session, the user and the `Ctx` with their role's
/// scopes, read fresh on every request (design note 0008, A3).
#[derive(Debug, Clone)]
pub struct StaffSession {
    pub ctx: Ctx,
    pub session: Session,
    pub user: User,
    pub role: Role,
}

/// Who `token` belongs to. Unknown, expired, revoked or pending sessions and disabled
/// users are all `invalid_token`.
///
/// One read: the session, its user and their role come together
/// (`StaffSessionRepo::staff_session_by_token`), so every staff request costs one round
/// trip before its own work; and a write once per `Session::USE_RESOLUTION` recording
/// that the session is in use.
pub async fn authenticate_staff<S: HasStaffSessions + HasSessions>(
    ports: &S,
    clock: &impl Clock,
    token: &str,
) -> Result<StaffSession, CommandError> {
    let hash = hypha_auth::session_token_hash(token)?;
    let StaffSessionRecord {
        mut session,
        user,
        role,
    } = ports
        .staff_sessions()
        .staff_session_by_token(hash)
        .await?
        .filter(|found| found.session.is_active(clock.now()))
        .ok_or(hypha_auth::CommandError::InvalidToken)?;
    let user = user.filter(User::is_active).ok_or(CommandError::NotStaff)?;
    let role = role.ok_or(CommandError::RoleNotFound(user.role()))?;
    record_session_use(ports.sessions(), clock, &mut session).await?;
    let ctx = Ctx::new(Actor::User(user.id()), role.granted());
    Ok(StaffSession {
        ctx,
        session,
        user,
        role,
    })
}

// ---------- password reset ----------

/// Emails a reset link if `email` belongs to an active staff member, and does nothing
/// otherwise. Callers answer the same either way, so nobody can probe for accounts.
///
/// The user is checked before a link is made, so a disabled account doesn't get a link
/// that would hold back the next request (`PasswordReset::blocks_another`); and a link
/// whose email couldn't be sent is withdrawn for the same reason.
pub async fn request_password_reset<S: StaffPorts>(
    ports: &S,
    env: &(impl AuthEnv + Notifier<StaffEmail>),
    email: &str,
) -> Result<(), CommandError> {
    let Ok(address) = Email::new(email) else {
        return Ok(());
    };
    // A staff member's login email is their user email.
    if !ports
        .users()
        .get_by_email(&address)
        .await?
        .is_some_and(|u| u.is_active())
    {
        return Ok(());
    }
    let Some((identity, reset, token)) =
        hypha_auth::start_password_reset(ports, env, PrincipalKind::Staff, email).await?
    else {
        return Ok(());
    };
    let sent = env
        .notify(StaffEmail::PasswordReset {
            to: address,
            token: token.expose().to_string(),
            expires_at: reset.expires_at(),
        })
        .await;
    if let Err(e) = sent {
        hypha_auth::withdraw_password_resets(ports, env, identity.id()).await?;
        return Err(e.into());
    }
    Ok(())
}
