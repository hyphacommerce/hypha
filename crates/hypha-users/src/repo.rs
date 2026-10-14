use hypha_auth::TokenHash;
use hypha_core::{Email, RepoError, RoleId, UserId};

use crate::{Invite, Role, User};

/// Uniqueness rules the repos enforce, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const USER_ID: &str = "user_id";
    pub const USER_EMAIL: &str = "user_email";
    pub const ROLE_ID: &str = "role_id";
    /// Role names are unique ignoring case.
    pub const ROLE_NAME: &str = "role_name";
    /// At most one owner role (`RoleScopes::All`).
    pub const ROLE_OWNER: &str = "role_owner";
    pub const INVITE_ID: &str = "invite_id";
    /// One invite per email; inviting again renews it.
    pub const INVITE_EMAIL: &str = "invite_email";
    pub const INVITE_TOKEN: &str = "invite_token";
}

#[allow(async_fn_in_trait)]
pub trait UserRepo {
    async fn get(&self, id: UserId) -> Result<Option<User>, RepoError>;
    async fn get_by_email(&self, email: &Email) -> Result<Option<User>, RepoError>;
    /// Whether any user exists (first start: create the owner).
    async fn any(&self) -> Result<bool, RepoError>;
    /// How many active users have `role` (the last-owner guard).
    async fn count_active_with_role(&self, role: RoleId) -> Result<usize, RepoError>;
    /// Enforces `USER_ID` and `USER_EMAIL`.
    async fn insert(&self, user: &mut User) -> Result<(), RepoError>;
    async fn update(&self, user: &mut User) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait RoleRepo {
    async fn get(&self, id: RoleId) -> Result<Option<Role>, RepoError>;
    /// The role named `name`, ignoring case.
    async fn get_by_name(&self, name: &str) -> Result<Option<Role>, RepoError>;
    /// The owner role, once created.
    async fn owner(&self) -> Result<Option<Role>, RepoError>;
    /// Enforces `ROLE_ID`, `ROLE_NAME` and `ROLE_OWNER`.
    async fn insert(&self, role: &mut Role) -> Result<(), RepoError>;
    /// Enforces `ROLE_NAME`.
    async fn update(&self, role: &mut Role) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait InviteRepo {
    async fn get_by_email(&self, email: &Email) -> Result<Option<Invite>, RepoError>;
    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<Invite>, RepoError>;
    /// Enforces `INVITE_ID`, `INVITE_EMAIL` and `INVITE_TOKEN`.
    async fn insert(&self, invite: &mut Invite) -> Result<(), RepoError>;
    /// Enforces `INVITE_TOKEN`.
    async fn update(&self, invite: &mut Invite) -> Result<(), RepoError>;
}

/// Stages user writes in a `hypha_core::HasBatch` batch.
pub trait StageUsers {
    /// What `UserRepo::insert` would write; same keys on commit.
    fn insert_user(&mut self, user: &mut User);
}

/// Stages role writes in a `hypha_core::HasBatch` batch.
pub trait StageRoles {
    /// What `RoleRepo::insert` would write; same keys on commit.
    fn insert_role(&mut self, role: &mut Role);
}

/// Stages invite writes in a `hypha_core::HasBatch` batch.
pub trait StageInvites {
    fn update_invite(&mut self, invite: &mut Invite);
}

pub trait HasUsers {
    type Users: UserRepo;
    fn users(&self) -> &Self::Users;
}

pub trait HasRoles {
    type Roles: RoleRepo;
    fn roles(&self) -> &Self::Roles;
}

pub trait HasInvites {
    type Invites: InviteRepo;
    fn invites(&self) -> &Self::Invites;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasUsers + ?Sized> HasUsers for &T {
    type Users = T::Users;
    fn users(&self) -> &Self::Users {
        (**self).users()
    }
}

impl<T: HasRoles + ?Sized> HasRoles for &T {
    type Roles = T::Roles;
    fn roles(&self) -> &Self::Roles {
        (**self).roles()
    }
}

impl<T: HasInvites + ?Sized> HasInvites for &T {
    type Invites = T::Invites;
    fn invites(&self) -> &Self::Invites {
        (**self).invites()
    }
}
