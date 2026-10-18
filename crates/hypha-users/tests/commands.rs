use hypha_auth::{LoginStep, PasswordError};
use hypha_core::{Actor, AuthError, Ctx, Email, NotifyError, RoleId, Scope, UserId, Versioned};
use hypha_testkit::{InMemoryPorts, TestEnv};
use hypha_users::{
    AcceptInvite, CommandError, InviteUser, NewOwner, Role, RoleError, RoleRepo, StaffEmail,
    UpdateRole, UpdateUser, User, UserRepo, UserStatus, accept_invite, authenticate_staff,
    bootstrap_owner, create_role, get_user, invite_user, login, request_password_reset,
    update_role, update_user,
};
use pollster::block_on;
use uuid::Uuid;

const OWNER_PASSWORD: &str = "owner password";

/// A store with its owner (ana@example.com).
fn world() -> (InMemoryPorts, TestEnv, User) {
    let ports = InMemoryPorts::default();
    let env = TestEnv::default();
    let owner = block_on(bootstrap_owner(
        &Ctx::system(),
        &ports,
        &env,
        NewOwner {
            email: "ana@example.com".to_string(),
            name: "Ana".to_string(),
            password: OWNER_PASSWORD.to_string(),
        },
    ))
    .unwrap()
    .unwrap();
    (ports, env, owner)
}

fn owner_ctx(owner: &User) -> Ctx {
    Ctx::new(Actor::User(owner.id()), Scope::ALL.to_vec())
}

fn session_token(step: LoginStep) -> String {
    match step {
        LoginStep::Authenticated { token, .. } => token.expose().to_string(),
        LoginStep::MfaRequired { .. } => panic!("no MFA in these tests"),
    }
}

fn support_role(ports: &InMemoryPorts, env: &TestEnv, owner: &User) -> Role {
    block_on(create_role(
        &owner_ctx(owner),
        &ports.roles,
        env,
        "Support",
        &[Scope::OrdersWrite, Scope::CustomersRead],
    ))
    .unwrap()
}

/// Invites bob@example.com with `role` and accepts; returns the user and their token.
fn join(ports: &InMemoryPorts, env: &TestEnv, owner: &User, role: RoleId) -> (User, String) {
    let input = InviteUser {
        email: "bob@example.com".to_string(),
        role,
    };
    block_on(invite_user(&owner_ctx(owner), ports, env, input)).unwrap();
    let accepted = AcceptInvite {
        token: env.last_token().unwrap(),
        name: "Bob".to_string(),
        password: "bob password".to_string(),
    };
    let (user, _, token) = block_on(accept_invite(ports, env, accepted)).unwrap();
    (user, token.expose().to_string())
}

// ---------- first start ----------

#[test]
fn the_first_start_creates_the_owner_who_can_log_in() {
    let (ports, env, owner) = world();
    let role = block_on(ports.roles.get(owner.role())).unwrap().unwrap();
    assert!(role.is_owner());
    let token =
        session_token(block_on(login(&ports, &env, "ana@example.com", OWNER_PASSWORD)).unwrap());
    let staff = block_on(authenticate_staff(&ports, &env, &token)).unwrap();
    assert_eq!(staff.user.id(), owner.id());
    assert_eq!(staff.ctx.actor(), Actor::User(owner.id()));
    assert!(Scope::ALL.iter().all(|s| staff.ctx.has_scope(*s)));
}

#[test]
fn the_owner_is_only_created_once() {
    let (ports, env, _) = world();
    let again = NewOwner {
        email: "eve@example.com".to_string(),
        name: "Eve".to_string(),
        password: "eve password".to_string(),
    };
    assert_eq!(
        block_on(bootstrap_owner(&Ctx::system(), &ports, &env, again)).map(|u| u.is_none()),
        Ok(true)
    );
    let eve = Email::new("eve@example.com").unwrap();
    assert_eq!(block_on(ports.users.get_by_email(&eve)), Ok(None));
}

#[test]
fn losing_the_race_to_create_the_owner_is_finding_them() {
    let (ports, env, _) = world();
    // Another instance created the owner between this one's check and its write.
    ports.users.hide_next_lookups(1);
    let again = NewOwner {
        email: "ana@example.com".to_string(),
        name: "Ana".to_string(),
        password: OWNER_PASSWORD.to_string(),
    };
    assert_eq!(
        block_on(bootstrap_owner(&Ctx::system(), &ports, &env, again)).map(|u| u.is_none()),
        Ok(true)
    );
}

#[test]
fn the_owner_password_follows_the_rules() {
    let ports = InMemoryPorts::default();
    let input = NewOwner {
        email: "ana@example.com".to_string(),
        name: "Ana".to_string(),
        password: "short".to_string(),
    };
    assert_eq!(
        block_on(bootstrap_owner(
            &Ctx::system(),
            &ports,
            &TestEnv::default(),
            input
        ))
        .map(|u| u.is_some()),
        Err(CommandError::Password(PasswordError::TooShort { min: 8 }))
    );
    assert_eq!(block_on(ports.users.any()), Ok(false));
}

// ---------- login and sessions ----------

#[test]
fn a_wrong_password_is_invalid_credentials() {
    let (ports, env, _) = world();
    assert_eq!(
        block_on(login(&ports, &env, "ana@example.com", "wrong password")).map(|_| ()),
        Err(CommandError::Login(
            hypha_auth::CommandError::InvalidCredentials
        ))
    );
}

#[test]
fn a_disabled_user_can_neither_log_in_nor_use_a_session() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let (bob, token) = join(&ports, &env, &owner, support.id());
    let disable = UpdateUser {
        status: Some(UserStatus::Disabled),
        ..UpdateUser::default()
    };
    block_on(update_user(
        &owner_ctx(&owner),
        &ports,
        &env,
        bob.id(),
        disable,
    ))
    .unwrap();

    // Disabling revoked the session outright.
    assert_eq!(
        block_on(authenticate_staff(&ports, &env, &token)).map(|s| s.user.id()),
        Err(CommandError::Login(hypha_auth::CommandError::InvalidToken))
    );
    assert_eq!(
        block_on(login(&ports, &env, "bob@example.com", "bob password")).map(|_| ()),
        Err(CommandError::Login(
            hypha_auth::CommandError::InvalidCredentials
        ))
    );
}

#[test]
fn enabling_a_disabled_user_again_does_not_bring_back_old_sessions() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let (bob, token) = join(&ports, &env, &owner, support.id());
    let status = |status| UpdateUser {
        status: Some(status),
        ..UpdateUser::default()
    };
    block_on(update_user(
        &owner_ctx(&owner),
        &ports,
        &env,
        bob.id(),
        status(UserStatus::Disabled),
    ))
    .unwrap();
    block_on(update_user(
        &owner_ctx(&owner),
        &ports,
        &env,
        bob.id(),
        status(UserStatus::Active),
    ))
    .unwrap();
    assert_eq!(
        block_on(authenticate_staff(&ports, &env, &token)).map(|s| s.user.id()),
        Err(CommandError::Login(hypha_auth::CommandError::InvalidToken))
    );
    // Logging in again works.
    assert!(block_on(login(&ports, &env, "bob@example.com", "bob password")).is_ok());
}

#[test]
fn disabling_a_user_uses_up_their_reset_links() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let (bob, _) = join(&ports, &env, &owner, support.id());
    block_on(request_password_reset(&ports, &env, "bob@example.com")).unwrap();
    let link = env.last_token().unwrap();
    let disable = UpdateUser {
        status: Some(UserStatus::Disabled),
        ..UpdateUser::default()
    };
    block_on(update_user(
        &owner_ctx(&owner),
        &ports,
        &env,
        bob.id(),
        disable,
    ))
    .unwrap();
    assert_eq!(
        block_on(hypha_auth::reset_password(
            &ports,
            &env,
            &link,
            "thief password"
        ))
        .map(|i| i.id()),
        Err(hypha_auth::CommandError::InvalidToken)
    );
}

#[test]
fn removing_an_owner_claims_the_owner_role() {
    let (ports, env, owner) = world();
    let (bob, _) = join(&ports, &env, &owner, owner.role());
    let before = block_on(ports.roles.get(owner.role())).unwrap().unwrap();
    let disable = UpdateUser {
        status: Some(UserStatus::Disabled),
        ..UpdateUser::default()
    };
    block_on(update_user(
        &owner_ctx(&owner),
        &ports,
        &env,
        bob.id(),
        disable,
    ))
    .unwrap();
    let after = block_on(ports.roles.get(owner.role())).unwrap().unwrap();
    // The same role, one version on: a concurrent removal based on the old version fails.
    assert_eq!(after.granted(), before.granted());
    assert_eq!(after.version(), before.version().next());
}

#[test]
fn a_role_change_applies_to_the_next_request() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let (_, token) = join(&ports, &env, &owner, support.id());
    let before = block_on(authenticate_staff(&ports, &env, &token)).unwrap();
    assert!(before.ctx.has_scope(Scope::OrdersRead));
    assert!(!before.ctx.has_scope(Scope::ProductsRead));

    let change = UpdateRole {
        scopes: Some(vec![Scope::ProductsRead]),
        ..UpdateRole::default()
    };
    block_on(update_role(
        &owner_ctx(&owner),
        &ports.roles,
        support.id(),
        change,
    ))
    .unwrap();
    let after = block_on(authenticate_staff(&ports, &env, &token)).unwrap();
    assert!(after.ctx.has_scope(Scope::ProductsRead));
    assert!(!after.ctx.has_scope(Scope::OrdersRead));
}

// ---------- invites ----------

#[test]
fn an_invite_emails_a_token_that_creates_the_user_and_logs_them_in() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let input = InviteUser {
        email: " Bob@Example.com ".to_string(),
        role: support.id(),
    };
    let invite = block_on(invite_user(&owner_ctx(&owner), &ports, &env, input)).unwrap();
    assert_eq!(invite.invited_by(), Actor::User(owner.id()));
    let sent = env.sent();
    let [
        StaffEmail::Invite {
            to,
            token,
            expires_at,
        },
    ] = sent.as_slice()
    else {
        panic!("expected one invite email, got {:?}", env.sent());
    };
    assert_eq!(to.as_str(), "bob@example.com");
    assert!(token.starts_with("hypha_inv_"));
    assert_eq!(*expires_at, invite.expires_at());

    let accepted = AcceptInvite {
        token: token.clone(),
        name: "Bob".to_string(),
        password: "bob password".to_string(),
    };
    let (user, session, session_token) = block_on(accept_invite(&ports, &env, accepted)).unwrap();
    assert_eq!(user.role(), support.id());
    let staff = block_on(authenticate_staff(&ports, &env, session_token.expose())).unwrap();
    assert_eq!(staff.session.id(), session.id());
    assert!(block_on(login(&ports, &env, "bob@example.com", "bob password")).is_ok());
}

#[test]
fn an_invite_token_works_once() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    join(&ports, &env, &owner, support.id());
    let again = AcceptInvite {
        token: env.last_token().unwrap(),
        name: "Bob".to_string(),
        password: "bob password".to_string(),
    };
    assert_eq!(
        block_on(accept_invite(&ports, &env, again)).map(|(u, ..)| u.id()),
        Err(CommandError::Login(hypha_auth::CommandError::InvalidToken))
    );
}

#[test]
fn inviting_again_replaces_the_token() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let invite = |role| InviteUser {
        email: "bob@example.com".to_string(),
        role,
    };
    let first = block_on(invite_user(
        &owner_ctx(&owner),
        &ports,
        &env,
        invite(support.id()),
    ))
    .unwrap();
    let old_token = env.last_token().unwrap();
    let owner_role = owner.role();
    let second = block_on(invite_user(
        &owner_ctx(&owner),
        &ports,
        &env,
        invite(owner_role),
    ))
    .unwrap();
    assert_eq!(first.id(), second.id());
    assert_eq!(second.role(), owner_role);

    let stale = AcceptInvite {
        token: old_token,
        name: "Bob".to_string(),
        password: "bob password".to_string(),
    };
    assert_eq!(
        block_on(accept_invite(&ports, &env, stale)).map(|(u, ..)| u.id()),
        Err(CommandError::Login(hypha_auth::CommandError::InvalidToken))
    );
}

#[test]
fn losing_the_race_to_invite_renews_the_winner() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let invite = || InviteUser {
        email: "bob@example.com".to_string(),
        role: support.id(),
    };
    let first = block_on(invite_user(&owner_ctx(&owner), &ports, &env, invite())).unwrap();
    // The second request's lookup ran before the first one's insert.
    ports.invites.hide_next_lookups(1);
    let second = block_on(invite_user(&owner_ctx(&owner), &ports, &env, invite())).unwrap();
    assert_eq!(second.id(), first.id());
    // The renewed invite's token is the one in the last email.
    let accepted = AcceptInvite {
        token: env.last_token().unwrap(),
        name: "Bob".to_string(),
        password: "bob password".to_string(),
    };
    assert!(block_on(accept_invite(&ports, &env, accepted)).is_ok());
}

#[test]
fn an_invite_expires_after_a_week() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let input = InviteUser {
        email: "bob@example.com".to_string(),
        role: support.id(),
    };
    block_on(invite_user(&owner_ctx(&owner), &ports, &env, input)).unwrap();
    env.advance_by(env.auth.invite_lifetime);
    let accepted = AcceptInvite {
        token: env.last_token().unwrap(),
        name: "Bob".to_string(),
        password: "bob password".to_string(),
    };
    assert_eq!(
        block_on(accept_invite(&ports, &env, accepted)).map(|(u, ..)| u.id()),
        Err(CommandError::Login(hypha_auth::CommandError::InvalidToken))
    );
}

#[test]
fn staff_members_cannot_be_invited() {
    let (ports, env, owner) = world();
    let input = InviteUser {
        email: "ana@example.com".to_string(),
        role: owner.role(),
    };
    assert_eq!(
        block_on(invite_user(&owner_ctx(&owner), &ports, &env, input)).map(|i| i.id()),
        Err(CommandError::UserExists(owner.email().clone()))
    );
}

#[test]
fn nobody_invites_with_a_role_stronger_than_their_own() {
    let (ports, env, owner) = world();
    let ctx = Ctx::new(Actor::System, vec![Scope::UsersWrite]);
    let input = InviteUser {
        email: "bob@example.com".to_string(),
        role: owner.role(),
    };
    assert_eq!(
        block_on(invite_user(&ctx, &ports, &env, input)).map(|i| i.id()),
        Err(CommandError::Login(hypha_auth::CommandError::CannotGrant(
            Scope::ProductsRead
        )))
    );
    assert!(env.sent().is_empty());
}

#[test]
fn a_failed_email_keeps_the_invite_and_inviting_again_resends() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let invite = || InviteUser {
        email: "bob@example.com".to_string(),
        role: support.id(),
    };
    env.fail_sending(true);
    assert_eq!(
        block_on(invite_user(&owner_ctx(&owner), &ports, &env, invite())).map(|i| i.id()),
        Err(CommandError::Notify(NotifyError(
            "the test made sending fail".to_string()
        )))
    );
    env.fail_sending(false);
    block_on(invite_user(&owner_ctx(&owner), &ports, &env, invite())).unwrap();
    assert_eq!(env.sent().len(), 1);
}

#[test]
fn accepting_follows_the_password_and_name_rules() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let input = InviteUser {
        email: "bob@example.com".to_string(),
        role: support.id(),
    };
    block_on(invite_user(&owner_ctx(&owner), &ports, &env, input)).unwrap();
    let accepted = AcceptInvite {
        token: env.last_token().unwrap(),
        name: "Bob".to_string(),
        password: "short".to_string(),
    };
    assert_eq!(
        block_on(accept_invite(&ports, &env, accepted)).map(|(u, ..)| u.id()),
        Err(CommandError::Password(PasswordError::TooShort { min: 8 }))
    );
    let bob = Email::new("bob@example.com").unwrap();
    assert_eq!(block_on(ports.users.get_by_email(&bob)), Ok(None));
}

// ---------- roles ----------

#[test]
fn role_names_are_unique_ignoring_case() {
    let (ports, env, owner) = world();
    support_role(&ports, &env, &owner);
    assert_eq!(
        block_on(create_role(
            &owner_ctx(&owner),
            &ports.roles,
            &env,
            "SUPPORT",
            &[Scope::OrdersRead]
        ))
        .map(|r| r.id()),
        Err(CommandError::RoleNameTaken("Support".to_string()))
    );
}

#[test]
fn a_race_for_a_role_name_reports_the_winner() {
    let (ports, env, owner) = world();
    support_role(&ports, &env, &owner);
    ports.roles.hide_next_lookups(1);
    assert_eq!(
        block_on(create_role(
            &owner_ctx(&owner),
            &ports.roles,
            &env,
            "support",
            &[Scope::OrdersRead]
        ))
        .map(|r| r.id()),
        Err(CommandError::RoleNameTaken("Support".to_string()))
    );
}

#[test]
fn the_owner_role_cannot_be_edited() {
    let (ports, _, owner) = world();
    let change = UpdateRole {
        name: Some("Boss".to_string()),
        ..UpdateRole::default()
    };
    assert_eq!(
        block_on(update_role(
            &owner_ctx(&owner),
            &ports.roles,
            owner.role(),
            change
        ))
        .map(|r| r.id()),
        Err(CommandError::Role(RoleError::OwnerRoleFixed))
    );
}

#[test]
fn nobody_creates_or_edits_a_role_beyond_their_scopes() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let limited = Ctx::new(Actor::System, vec![Scope::UsersWrite, Scope::OrdersRead]);
    assert_eq!(
        block_on(create_role(
            &limited,
            &ports.roles,
            &env,
            "Writers",
            &[Scope::OrdersWrite]
        ))
        .map(|r| r.id()),
        Err(CommandError::Login(hypha_auth::CommandError::CannotGrant(
            Scope::OrdersWrite
        )))
    );
    // Support has scopes `limited` lacks, so it can't touch it at all.
    let change = UpdateRole {
        scopes: Some(vec![Scope::OrdersRead]),
        ..UpdateRole::default()
    };
    assert_eq!(
        block_on(update_role(&limited, &ports.roles, support.id(), change)).map(|r| r.id()),
        Err(CommandError::Login(hypha_auth::CommandError::CannotGrant(
            Scope::CustomersRead
        )))
    );
}

// ---------- users ----------

#[test]
fn the_last_owner_stays_an_active_owner() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    for change in [
        UpdateUser {
            status: Some(UserStatus::Disabled),
            ..UpdateUser::default()
        },
        UpdateUser {
            role: Some(support.id()),
            ..UpdateUser::default()
        },
    ] {
        assert_eq!(
            block_on(update_user(
                &owner_ctx(&owner),
                &ports,
                &env,
                owner.id(),
                change
            ))
            .map(|u| u.id()),
            Err(CommandError::LastOwner)
        );
    }
}

#[test]
fn an_owner_can_step_down_once_another_exists() {
    let (ports, env, owner) = world();
    let (bob, _) = join(&ports, &env, &owner, owner.role());
    let support = support_role(&ports, &env, &owner);
    let step_down = UpdateUser {
        role: Some(support.id()),
        ..UpdateUser::default()
    };
    let ana = block_on(update_user(
        &owner_ctx(&owner),
        &ports,
        &env,
        owner.id(),
        step_down,
    ))
    .unwrap();
    assert_eq!(ana.role(), support.id());
    // Now Bob is the last one.
    let disable = UpdateUser {
        status: Some(UserStatus::Disabled),
        ..UpdateUser::default()
    };
    assert_eq!(
        block_on(update_user(
            &owner_ctx(&owner),
            &ports,
            &env,
            bob.id(),
            disable
        ))
        .map(|u| u.id()),
        Err(CommandError::LastOwner)
    );
}

#[test]
fn nobody_disables_someone_stronger() {
    let (ports, env, owner) = world();
    let limited = Ctx::new(Actor::System, vec![Scope::UsersWrite]);
    let disable = UpdateUser {
        status: Some(UserStatus::Disabled),
        ..UpdateUser::default()
    };
    assert_eq!(
        block_on(update_user(&limited, &ports, &env, owner.id(), disable)).map(|u| u.id()),
        Err(CommandError::Login(hypha_auth::CommandError::CannotGrant(
            Scope::ProductsRead
        )))
    );
    // Renaming changes no power, so it is fine.
    let rename = UpdateUser {
        name: Some("Ana B.".to_string()),
        ..UpdateUser::default()
    };
    assert_eq!(
        block_on(update_user(&limited, &ports, &env, owner.id(), rename))
            .map(|u| u.name().to_string()),
        Ok("Ana B.".to_string())
    );
}

#[test]
fn reading_a_user_needs_users_read() {
    let (ports, _, owner) = world();
    let ctx = Ctx::new(Actor::System, vec![Scope::OrdersRead]);
    assert_eq!(
        block_on(get_user(&ctx, &ports.users, owner.id())).map(|u| u.id()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::UsersRead
        )))
    );
    let unknown = UserId::from_uuid(Uuid::from_u128(999));
    assert_eq!(
        block_on(get_user(&Ctx::system(), &ports.users, unknown)).map(|u| u.id()),
        Err(CommandError::UserNotFound(unknown))
    );
}

// ---------- password reset ----------

#[test]
fn a_reset_is_emailed_to_staff_only() {
    let (ports, env, _) = world();
    block_on(request_password_reset(&ports, &env, "nobody@example.com")).unwrap();
    assert!(env.sent().is_empty());
    block_on(request_password_reset(&ports, &env, "ana@example.com")).unwrap();
    let sent = env.sent();
    let [StaffEmail::PasswordReset { to, token, .. }] = sent.as_slice() else {
        panic!("expected one reset email, got {:?}", env.sent());
    };
    assert_eq!(to.as_str(), "ana@example.com");
    block_on(hypha_auth::reset_password(
        &ports,
        &env,
        token,
        "brand new pass",
    ))
    .unwrap();
    assert!(block_on(login(&ports, &env, "ana@example.com", "brand new pass")).is_ok());
}

#[test]
fn a_reset_email_that_failed_does_not_hold_back_the_next() {
    let (ports, env, _) = world();
    env.fail_sending(true);
    assert!(matches!(
        block_on(request_password_reset(&ports, &env, "ana@example.com")),
        Err(CommandError::Notify(_))
    ));
    env.fail_sending(false);
    block_on(request_password_reset(&ports, &env, "ana@example.com")).unwrap();
    assert_eq!(env.sent().len(), 1);
}

#[test]
fn a_reset_asked_while_disabled_does_not_hold_back_the_next() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let (bob, _) = join(&ports, &env, &owner, support.id());
    let status = |status| UpdateUser {
        status: Some(status),
        ..UpdateUser::default()
    };
    block_on(update_user(
        &owner_ctx(&owner),
        &ports,
        &env,
        bob.id(),
        status(UserStatus::Disabled),
    ))
    .unwrap();
    let before = env.sent().len();
    block_on(request_password_reset(&ports, &env, "bob@example.com")).unwrap();
    block_on(update_user(
        &owner_ctx(&owner),
        &ports,
        &env,
        bob.id(),
        status(UserStatus::Active),
    ))
    .unwrap();
    block_on(request_password_reset(&ports, &env, "bob@example.com")).unwrap();
    assert_eq!(env.sent().len(), before + 1);
}

#[test]
fn a_disabled_user_gets_no_reset_email() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let (bob, _) = join(&ports, &env, &owner, support.id());
    let disable = UpdateUser {
        status: Some(UserStatus::Disabled),
        ..UpdateUser::default()
    };
    block_on(update_user(
        &owner_ctx(&owner),
        &ports,
        &env,
        bob.id(),
        disable,
    ))
    .unwrap();
    let before = env.sent().len();
    block_on(request_password_reset(&ports, &env, "bob@example.com")).unwrap();
    assert_eq!(env.sent().len(), before);
}

// ---------- a disable landing in the middle of a login ----------

/// Ports whose user lookups disable the user right after the first one answered: as if
/// an admin's disable committed between login's check and its session insert. The
/// login's batch then conflicts on the user, and its second look sees the disable.
struct DisableAfterFirstLookup<'a> {
    ports: &'a InMemoryPorts,
    users: DisablingUsers<'a>,
}

struct DisablingUsers<'a> {
    inner: &'a hypha_testkit::InMemoryUserRepo,
    looked_up: std::cell::Cell<bool>,
}

impl UserRepo for DisablingUsers<'_> {
    async fn get(&self, id: UserId) -> Result<Option<User>, hypha_core::RepoError> {
        let found = self.inner.get(id).await?;
        if !self.looked_up.replace(true)
            && let Some(mut user) = found.clone()
        {
            user.set_status(UserStatus::Disabled);
            self.inner.update(&mut user).await?;
        }
        Ok(found)
    }
    async fn get_by_email(&self, email: &Email) -> Result<Option<User>, hypha_core::RepoError> {
        self.inner.get_by_email(email).await
    }
    async fn any(&self) -> Result<bool, hypha_core::RepoError> {
        self.inner.any().await
    }
    async fn list_all(&self) -> Result<Vec<User>, hypha_core::RepoError> {
        self.inner.list_all().await
    }
    async fn count_active_with_role(&self, role: RoleId) -> Result<usize, hypha_core::RepoError> {
        self.inner.count_active_with_role(role).await
    }
    async fn insert(&self, user: &mut User) -> Result<(), hypha_core::RepoError> {
        self.inner.insert(user).await
    }
    async fn update(&self, user: &mut User) -> Result<(), hypha_core::RepoError> {
        self.inner.update(user).await
    }
}

impl<'a> hypha_users::HasUsers for DisableAfterFirstLookup<'a> {
    type Users = DisablingUsers<'a>;
    fn users(&self) -> &DisablingUsers<'a> {
        &self.users
    }
}

impl hypha_auth::HasIdentities for DisableAfterFirstLookup<'_> {
    type Identities = hypha_testkit::InMemoryIdentityRepo;
    fn identities(&self) -> &Self::Identities {
        &self.ports.identities
    }
}

impl hypha_auth::HasSessions for DisableAfterFirstLookup<'_> {
    type Sessions = hypha_testkit::InMemorySessionRepo;
    fn sessions(&self) -> &Self::Sessions {
        &self.ports.sessions
    }
}

impl hypha_auth::HasPasswordResets for DisableAfterFirstLookup<'_> {
    type PasswordResets = hypha_testkit::InMemoryPasswordResetRepo;
    fn password_resets(&self) -> &Self::PasswordResets {
        &self.ports.password_resets
    }
}

impl hypha_users::HasRoles for DisableAfterFirstLookup<'_> {
    type Roles = hypha_testkit::InMemoryRoleRepo;
    fn roles(&self) -> &Self::Roles {
        &self.ports.roles
    }
}

impl hypha_users::HasInvites for DisableAfterFirstLookup<'_> {
    type Invites = hypha_testkit::InMemoryInviteRepo;
    fn invites(&self) -> &Self::Invites {
        &self.ports.invites
    }
}

/// Batches go to the real tables: the disable the user lookup simulated is already
/// there when the login's batch commits.
impl hypha_core::HasBatch for DisableAfterFirstLookup<'_> {
    type Batch = hypha_testkit::InMemoryBatch;
    fn batch(&self) -> Self::Batch {
        self.ports.batch()
    }
    async fn commit(&self, batch: Self::Batch) -> Result<(), hypha_core::RepoError> {
        self.ports.commit(batch).await
    }
}

#[test]
fn a_disable_during_login_leaves_no_session_to_revive() {
    let (ports, env, owner) = world();
    let racing = DisableAfterFirstLookup {
        ports: &ports,
        users: DisablingUsers {
            inner: &ports.users,
            looked_up: std::cell::Cell::new(false),
        },
    };
    assert_eq!(
        block_on(login(&racing, &env, "ana@example.com", OWNER_PASSWORD)).map(|_| ()),
        Err(CommandError::Login(
            hypha_auth::CommandError::InvalidCredentials
        ))
    );
    // Enabled again (directly: the disable left no other owner to do it), no session
    // from that login works.
    let mut ana = block_on(ports.users.get(owner.id())).unwrap().unwrap();
    ana.set_status(UserStatus::Active);
    block_on(ports.users.update(&mut ana)).unwrap();
    let identity = block_on(hypha_auth::IdentityRepo::get_by_principal(
        &ports.identities,
        hypha_auth::Principal::Staff(owner.id()),
    ))
    .unwrap()
    .unwrap();
    let live = ports.sessions.live_for(identity.id(), env.now());
    assert_eq!(live, 0, "a session from the login survived");
}

// ---------- authenticating a request ----------

#[test]
fn a_token_that_is_not_a_session_token_is_invalid() {
    let (ports, env, _) = world();
    assert_eq!(
        block_on(authenticate_staff(&ports, &env, "hypha_sk_nope")).map(|s| s.user.id()),
        Err(CommandError::Login(hypha_auth::CommandError::InvalidToken))
    );
    assert_eq!(
        block_on(authenticate_staff(&ports, &env, "hypha_sess_unknown")).map(|s| s.user.id()),
        Err(CommandError::Login(hypha_auth::CommandError::InvalidToken))
    );
}

#[test]
fn a_live_session_of_a_disabled_user_is_not_staff() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let (mut bob, token) = join(&ports, &env, &owner, support.id());
    // Disabled behind the commands' back, so the session stays live.
    bob.set_status(UserStatus::Disabled);
    block_on(ports.users.update(&mut bob)).unwrap();
    assert_eq!(
        block_on(authenticate_staff(&ports, &env, &token)).map(|s| s.user.id()),
        Err(CommandError::NotStaff)
    );
}

#[test]
fn a_user_whose_role_is_gone_is_role_not_found() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let (mut bob, token) = join(&ports, &env, &owner, support.id());
    let gone = RoleId::from_uuid(Uuid::from_u128(999));
    bob.set_role(gone);
    block_on(ports.users.update(&mut bob)).unwrap();
    assert_eq!(
        block_on(authenticate_staff(&ports, &env, &token)).map(|s| s.user.id()),
        Err(CommandError::RoleNotFound(gone))
    );
}

#[test]
fn a_request_reads_the_role_scopes_as_they_are_now() {
    let (ports, env, owner) = world();
    let support = support_role(&ports, &env, &owner);
    let (_, token) = join(&ports, &env, &owner, support.id());
    let staff = block_on(authenticate_staff(&ports, &env, &token)).unwrap();
    assert!(!staff.ctx.has_scope(Scope::ProductsRead));
    let mut role = block_on(ports.roles.get(support.id())).unwrap().unwrap();
    role.set_scopes(&[Scope::ProductsRead]).unwrap();
    block_on(ports.roles.update(&mut role)).unwrap();
    let staff = block_on(authenticate_staff(&ports, &env, &token)).unwrap();
    assert!(staff.ctx.has_scope(Scope::ProductsRead));
    assert!(!staff.ctx.has_scope(Scope::OrdersWrite));
}
