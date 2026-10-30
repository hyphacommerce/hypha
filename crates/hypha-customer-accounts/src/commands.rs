//! Signing in with an email code and being signed in.

use core::time::Duration;

use hypha_auth::{
    AuthEnv, CodeCheck, CredentialKind, HasIdentities, HasSessions, Identity, IdentityError,
    IdentityRepo, NewToken, Principal, PrincipalKind, Session, SessionClient, SessionRepo,
    StageIdentities, StageSessions, TokenHash, TokenKind, keys,
};
use hypha_cart::{Cart, CartRepo, HasCarts, StageCarts};
use hypha_core::{
    Actor, AuthError, CartId, ChannelId, Clock, Ctx, CustomerId, Email, ErrorCode, HasBatch,
    IdentityId, Notifier, NotifyError, RepoError, Scope, Timestamp,
};
use hypha_customers::{Customer, CustomerRepo, HasCustomers, find_or_create_customer};
use thiserror::Error;

use crate::settings::HasAccountSettings;

/// What the commands send: the host's `MailEnv` renders and sends it at once (the code
/// is a secret and isn't kept in a record), like `hypha_users::StaffEmail`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomerEmail {
    LoginCode {
        to: Email,
        code: String,
        expires_at: Timestamp,
    },
    /// The person asked from their account for their data to be erased: when it runs, so
    /// they can still cancel.
    ErasureRequested { to: Email, due_at: Timestamp },
}

/// The non-storage ports: auth's, the account settings and the sender.
pub trait AccountEnv: AuthEnv + HasAccountSettings + Notifier<CustomerEmail> {}
impl<T: AuthEnv + HasAccountSettings + Notifier<CustomerEmail>> AccountEnv for T {}

/// The repositories the flows write: identities and sessions, the customer, the cart
/// linked at login, in one batch.
pub trait AccountPorts:
    HasIdentities
    + HasSessions
    + HasCustomers
    + HasCarts
    + HasBatch<Batch: StageIdentities + StageSessions + StageCarts>
{
}
impl<T> AccountPorts for T where
    T: HasIdentities
        + HasSessions
        + HasCustomers
        + HasCarts
        + HasBatch<Batch: StageIdentities + StageSessions + StageCarts>
{
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error(transparent)]
    Customers(#[from] hypha_customers::CommandError),
    #[error(transparent)]
    Notify(#[from] NotifyError),
    /// Wrong, expired or used up: one answer, so a guesser learns nothing.
    #[error("the code is wrong or has expired")]
    InvalidCode,
    /// A session token that is malformed, unknown, expired or revoked.
    #[error("the token is invalid or expired")]
    InvalidToken,
    /// The session's customer is gone (erased): sign in again.
    #[error("customer {0} not found")]
    CustomerNotFound(CustomerId),
    /// Too many rounds lost to concurrent writes of the same identity.
    #[error("the account changed while signing in; try again")]
    Concurrent,
    /// An account route called by someone who isn't a signed-in customer.
    #[error("sign in first")]
    AccountRequired,
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Identity(e) => e.code(),
            CommandError::Customers(e) => e.code(),
            CommandError::Notify(e) => e.code(),
            CommandError::InvalidCode => "invalid_code",
            CommandError::InvalidToken => "invalid_token",
            CommandError::CustomerNotFound(_) => "customer_not_found",
            CommandError::Concurrent => "concurrent_modification",
            CommandError::AccountRequired => "account_required",
        }
    }
}

/// A signed-in customer's own record: the context to read and change it with, and who they
/// are. Every account command takes the customer from here, never from input: the context
/// has the scopes the customer's own record needs (`customers:read|write|erase`,
/// `orders:read`, every `customer_data:*`, so the person sees everything about themselves),
/// for this customer only, since the handler passes `customer` to every command.
#[derive(Debug, Clone)]
pub struct Owned {
    pub ctx: Ctx,
    pub customer: CustomerId,
}

/// The customer behind `ctx`, or `AccountRequired`.
pub fn own(ctx: &Ctx) -> Result<Owned, CommandError> {
    let Some(customer) = ctx.customer() else {
        return Err(CommandError::AccountRequired);
    };
    let mut scopes = Scope::STOREFRONT.to_vec();
    scopes.extend([
        Scope::CustomersRead,
        Scope::CustomersWrite,
        Scope::CustomersErase,
        Scope::OrdersRead,
    ]);
    scopes.extend(Scope::CUSTOMER_DATA);
    let owned = match ctx.channel() {
        Some(channel) => Ctx::in_channel(ctx.actor(), scopes, channel),
        None => Ctx::new(ctx.actor(), scopes),
    };
    Ok(Owned {
        ctx: owned,
        customer,
    })
}

/// How many concurrent-write rounds a flow tries before giving up.
const ROUNDS: usize = 3;

/// A code is six digits; the hash stored is of `<identity id>:<code>`, so the same code
/// hashes differently per account.
fn new_code(random: &impl hypha_core::Random) -> String {
    let mut bytes = [0u8; 4];
    random.fill(&mut bytes);
    format!("{:06}", u32::from_be_bytes(bytes) % 1_000_000)
}

fn code_hash(identity: IdentityId, code: &str) -> TokenHash {
    TokenHash::of(&format!("{identity}:{}", code.trim()))
}

/// The internal context for creating a customer: the shopper hasn't proven who they
/// are yet, and the storefront's scopes don't include `customers:write`.
fn creator() -> Ctx {
    Ctx::new(Actor::System, vec![Scope::CustomersWrite])
}

/// Emails a sign-in code to `email`, creating the customer and their identity if there are
/// none. Does nothing for an address that isn't one, a placeholder (an erased customer's),
/// or while the latest code is younger than `login_code_resend_after`: the route answers
/// the same in every case, so nobody can probe for accounts.
pub async fn request_login_code<S: AccountPorts>(
    ports: &S,
    env: &impl AccountEnv,
    email: &str,
) -> Result<(), CommandError> {
    let Ok(email) = Email::new(email) else {
        return Ok(());
    };
    if email.is_placeholder() {
        return Ok(());
    }
    let settings = *env.account_settings();
    for _ in 0..ROUNDS {
        let Some(mut identity) = find_identity(ports, env, &email).await? else {
            continue;
        };
        let now = env.now();
        if let Some(issued) = identity.code_issued_at(settings.login_code_lifetime)
            && now < issued.plus(settings.login_code_resend_after)
        {
            return Ok(());
        }
        let code = new_code(env);
        let expires_at = now.plus(settings.login_code_lifetime);
        identity.set_email_code(code_hash(identity.id(), &code), expires_at)?;
        match ports.identities().update(&mut identity).await {
            Ok(()) => {}
            Err(RepoError::StaleVersion) => continue,
            Err(e) => return Err(e.into()),
        }
        env.notify(CustomerEmail::LoginCode {
            to: email,
            code,
            expires_at,
        })
        .await?;
        return Ok(());
    }
    Err(CommandError::Concurrent)
}

/// The identity signing in with `email`, made if there's none: the customer with that
/// email (created when there's none), then their identity (created, or its code email
/// moved to `email` when the customer's address changed since). `None` when a race
/// created either first: the caller looks again.
async fn find_identity<S: AccountPorts>(
    ports: &S,
    env: &impl AccountEnv,
    email: &Email,
) -> Result<Option<Identity>, CommandError> {
    if let Some(found) = ports
        .identities()
        .find_login(
            PrincipalKind::Customer,
            CredentialKind::EmailCode,
            email.as_str(),
        )
        .await?
    {
        return match owner_of(ports, &found).await? {
            Owner::Current(_) => Ok(Some(found)),
            // The customer's address changed since (staff edit it; nothing else touches the
            // identity): the lookup follows it before this address can be anyone else's, so
            // the old mailbox can't open their account.
            Owner::Moved(current) => {
                let mut stale = found;
                stale.set_code_email(current)?;
                match ports.identities().update(&mut stale).await {
                    Ok(())
                    | Err(RepoError::StaleVersion | RepoError::Duplicate(keys::IDENTITY_LOGIN)) => {
                        Ok(None)
                    }
                    Err(e) => Err(e.into()),
                }
            }
            // Its customer is gone (the privacy batch takes the identity with it, so
            // only an older row): dropped, so the address is free.
            Owner::Gone => {
                let mut batch = ports.batch();
                batch.delete_identity(&found);
                match ports.commit(batch).await {
                    Ok(()) | Err(RepoError::StaleVersion) => Ok(None),
                    Err(e) => Err(e.into()),
                }
            }
        };
    }
    let customer = find_or_create_customer(&creator(), ports.customers(), env, email).await?;
    let principal = Principal::Customer(customer.id());
    match ports.identities().get_by_principal(principal).await? {
        Some(mut existing) => {
            existing.set_code_email(email.clone())?;
            match ports.identities().update(&mut existing).await {
                Ok(()) => Ok(Some(existing)),
                Err(RepoError::StaleVersion | RepoError::Duplicate(keys::IDENTITY_LOGIN)) => {
                    Ok(None)
                }
                Err(e) => Err(e.into()),
            }
        }
        None => {
            let mut identity = Identity::with_email_code(
                IdentityId::from_uuid(env.new_uuid()),
                principal,
                email.clone(),
            );
            match ports.identities().insert(&mut identity).await {
                Ok(()) => Ok(Some(identity)),
                Err(RepoError::Duplicate(_)) => Ok(None),
                Err(e) => Err(e.into()),
            }
        }
    }
}

/// Whether an identity found by its code email still belongs to a customer with that
/// address.
enum Owner {
    /// The customer, whose email is the identity's (boxed: the other variants are small).
    Current(Box<Customer>),
    /// The customer's email is another now: the identity's lookup is stale.
    Moved(Email),
    /// No customer, or an erased one (a placeholder address can't sign in).
    Gone,
}

async fn owner_of<S: HasCustomers>(ports: &S, identity: &Identity) -> Result<Owner, RepoError> {
    let Principal::Customer(id) = identity.principal() else {
        return Ok(Owner::Gone);
    };
    let Some(customer) = ports.customers().get(id).await?.filter(|c| !c.is_erased()) else {
        return Ok(Owner::Gone);
    };
    if identity.code_email() == Some(customer.email()) {
        Ok(Owner::Current(Box::new(customer)))
    } else {
        Ok(Owner::Moved(customer.email().clone()))
    }
}

/// A customer signed in: their session, its token (shown once) and who they are.
#[derive(Debug)]
pub struct LoggedIn {
    pub session: Session,
    pub token: NewToken,
    pub customer: Customer,
}

/// Signs in with the code emailed to `email`: the code is used up (right or wrong, so every
/// guess counts, AC1) and an active session starts, lasting `session_lifetime`. `cart` (the
/// request's, in `channel`) is linked to the customer in the same batch; a cart that isn't
/// theirs to use is left alone.
pub async fn login_with_code<S: AccountPorts>(
    ports: &S,
    env: &impl AccountEnv,
    channel: ChannelId,
    email: &str,
    code: &str,
    client: &SessionClient,
    cart: Option<CartId>,
) -> Result<LoggedIn, CommandError> {
    let Ok(email) = Email::new(email) else {
        return Err(CommandError::InvalidCode);
    };
    for _ in 0..ROUNDS {
        let Some(mut identity) = ports
            .identities()
            .find_login(
                PrincipalKind::Customer,
                CredentialKind::EmailCode,
                email.as_str(),
            )
            .await?
        else {
            return Err(CommandError::InvalidCode);
        };
        // A code only opens the account whose customer has this address now: an
        // identity left behind by an email change answers like an unknown one.
        let Owner::Current(customer) = owner_of(ports, &identity).await? else {
            return Err(CommandError::InvalidCode);
        };
        let customer = *customer;
        let now = env.now();
        let check = identity.use_email_code(code_hash(identity.id(), code), now);
        if check != CodeCheck::Right {
            // The guess counts even when the answer is no.
            match ports.identities().update(&mut identity).await {
                Ok(()) | Err(RepoError::StaleVersion) => {}
                Err(e) => return Err(e.into()),
            }
            return Err(CommandError::InvalidCode);
        }
        let token = NewToken::generate(TokenKind::CustomerSession, env);
        let mut session = Session::active(
            hypha_core::SessionId::from_uuid(env.new_uuid()),
            identity.id(),
            identity.principal(),
            token.hash(),
            now,
            None,
            env.account_settings().session_lifetime,
        );
        session.set_client(client.clone());
        let mut batch = ports.batch();
        batch.update_identity(&mut identity);
        batch.insert_session(&mut session);
        let mut linked = match cart {
            Some(id) => linkable_cart(ports, channel, id, &customer).await?,
            None => None,
        };
        if let Some(cart) = &mut linked {
            batch.update_cart(cart);
        }
        match ports.commit(batch).await {
            Ok(()) => {
                return Ok(LoggedIn {
                    session,
                    token,
                    customer,
                });
            }
            // Another request used the code or changed the cart meanwhile: look again
            // (a used code then answers `InvalidCode`).
            Err(RepoError::StaleVersion) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(CommandError::Concurrent)
}

/// The cart to link at login, set to `customer`: open, in `channel`, not already theirs.
async fn linkable_cart<S: AccountPorts>(
    ports: &S,
    channel: ChannelId,
    id: CartId,
    customer: &Customer,
) -> Result<Option<Cart>, CommandError> {
    let Some(mut cart) = ports.carts().get(id).await? else {
        return Ok(None);
    };
    if cart.channel() != channel
        || !matches!(cart.status(), hypha_cart::CartStatus::Open)
        || cart.customer() == Some(customer.id())
    {
        return Ok(None);
    }
    // A cart that can't take this buyer (its rules) is left as it was.
    if cart
        .set_customer(Some((customer.id(), customer.email().clone())))
        .is_err()
    {
        return Ok(None);
    }
    Ok(Some(cart))
}

/// A signed-in customer's request: the context to run storefront commands as them.
#[derive(Debug)]
pub struct CustomerSession {
    pub ctx: Ctx,
    pub session: Session,
    pub customer: Customer,
}

/// How much of a session's life must have passed before a use pushes its end out
/// again: a busy session is rewritten about daily, not per request, and a short
/// lifetime (under four days) slides a quarter at a time, so every use within it
/// still starts the count again.
fn extend_every(lifetime: Duration) -> Duration {
    const DAILY: Duration = Duration::from_secs(24 * 60 * 60);
    DAILY.min(lifetime / 4)
}

/// The customer behind a `hypha_cs_` token in `channel`, or `InvalidToken`. Using the
/// session keeps it alive: its end moves to `now + session_lifetime` about daily, and its
/// last use is recorded between.
pub async fn authenticate_customer<S: HasSessions + HasCustomers>(
    ports: &S,
    env: &(impl Clock + HasAccountSettings),
    channel: ChannelId,
    token: &str,
) -> Result<CustomerSession, CommandError> {
    if TokenKind::of(token) != Some(TokenKind::CustomerSession) {
        return Err(CommandError::InvalidToken);
    }
    let now = env.now();
    let mut session = ports
        .sessions()
        .get_by_token(TokenHash::of(token))
        .await?
        .filter(|s| s.is_active(now))
        .ok_or(CommandError::InvalidToken)?;
    let Principal::Customer(id) = session.principal() else {
        return Err(CommandError::InvalidToken);
    };
    let customer = ports
        .customers()
        .get(id)
        .await?
        .filter(|c| !c.is_erased())
        .ok_or(CommandError::CustomerNotFound(id))?;
    let lifetime = env.account_settings().session_lifetime;
    if session.extend(now, lifetime, extend_every(lifetime)) {
        session.record_use(now);
        match ports.sessions().update(&mut session).await {
            // Two requests at once: the other one moved it.
            Ok(()) | Err(RepoError::StaleVersion) => {}
            Err(e) => return Err(e.into()),
        }
    } else if session.use_is_stale(now) {
        ports.sessions().record_use(session.id(), now).await?;
        session.record_use(now);
    }
    let ctx = Ctx::in_channel(Actor::Customer(id), Scope::STOREFRONT.to_vec(), channel);
    Ok(CustomerSession {
        ctx,
        session,
        customer,
    })
}

/// Ends the current session.
pub async fn logout<S: HasSessions>(
    ports: &S,
    clock: &impl Clock,
    session: Session,
) -> Result<(), CommandError> {
    hypha_auth::logout(ports.sessions(), clock, session)
        .await
        .map_err(auth_error)
}

/// Ends every session of the customer, this one included.
pub async fn logout_all<S: HasBatch<Batch: StageSessions>>(
    ports: &S,
    clock: &impl Clock,
    session: &Session,
) -> Result<(), CommandError> {
    let mut batch = ports.batch();
    batch.revoke_sessions(session.identity(), None, clock.now());
    ports.commit(batch).await?;
    Ok(())
}

/// Auth's session commands can only fail on storage here.
fn auth_error(e: hypha_auth::CommandError) -> CommandError {
    match e {
        hypha_auth::CommandError::Repo(e) => CommandError::Repo(e),
        other => CommandError::Repo(RepoError::Backend(other.to_string())),
    }
}
