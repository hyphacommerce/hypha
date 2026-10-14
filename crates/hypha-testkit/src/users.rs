//! In-memory repos for `hypha-users`.

use hypha_auth::TokenHash;
use hypha_core::{Email, InviteId, RepoError, RoleId, UserId};
use hypha_users::{Invite, InviteRepo, Role, RoleRepo, User, UserRepo, keys};

use crate::table::Table;

#[derive(Default)]
pub struct InMemoryUserRepo {
    pub(crate) users: Table<UserId, User>,
}

pub(crate) fn unique_user(new: &User, other: &User) -> Option<&'static str> {
    (new.email() == other.email()).then_some(keys::USER_EMAIL)
}

impl UserRepo for InMemoryUserRepo {
    async fn get(&self, id: UserId) -> Result<Option<User>, RepoError> {
        Ok(self.users.get(&id))
    }

    async fn get_by_email(&self, email: &Email) -> Result<Option<User>, RepoError> {
        Ok(self.users.find(|u| u.email() == email))
    }

    async fn any(&self) -> Result<bool, RepoError> {
        Ok(!self.users.all().is_empty())
    }

    async fn count_active_with_role(&self, role: RoleId) -> Result<usize, RepoError> {
        Ok(self
            .users
            .all()
            .iter()
            .filter(|u| u.role() == role && u.is_active())
            .count())
    }

    async fn insert(&self, user: &mut User) -> Result<(), RepoError> {
        self.users
            .insert(user.id(), user, keys::USER_ID, &unique_user)
    }

    async fn update(&self, user: &mut User) -> Result<(), RepoError> {
        self.users.update(user.id(), user, &unique_user)
    }
}

#[derive(Default)]
pub struct InMemoryRoleRepo {
    pub(crate) roles: Table<RoleId, Role>,
}

/// Names unique ignoring case; one owner role.
pub(crate) fn unique_role(new: &Role, other: &Role) -> Option<&'static str> {
    if new.name().to_lowercase() == other.name().to_lowercase() {
        return Some(keys::ROLE_NAME);
    }
    (new.is_owner() && other.is_owner()).then_some(keys::ROLE_OWNER)
}

impl InMemoryRoleRepo {
    /// The next `n` lookups by name (or of the owner role) find nothing, as if another
    /// request inserted the role right after: for losing the race between a command's
    /// pre-check and its write.
    pub fn hide_next_lookups(&self, n: usize) {
        self.roles.hide_next_lookups(n);
    }
}

impl RoleRepo for InMemoryRoleRepo {
    async fn get(&self, id: RoleId) -> Result<Option<Role>, RepoError> {
        Ok(self.roles.get(&id))
    }

    async fn get_by_name(&self, name: &str) -> Result<Option<Role>, RepoError> {
        let name = name.trim().to_lowercase();
        Ok(self.roles.find(|r| r.name().to_lowercase() == name))
    }

    async fn owner(&self) -> Result<Option<Role>, RepoError> {
        Ok(self.roles.find(Role::is_owner))
    }

    async fn insert(&self, role: &mut Role) -> Result<(), RepoError> {
        self.roles
            .insert(role.id(), role, keys::ROLE_ID, &unique_role)
    }

    async fn update(&self, role: &mut Role) -> Result<(), RepoError> {
        self.roles.update(role.id(), role, &unique_role)
    }
}

#[derive(Default)]
pub struct InMemoryInviteRepo {
    pub(crate) invites: Table<InviteId, Invite>,
}

pub(crate) fn unique_invite(new: &Invite, other: &Invite) -> Option<&'static str> {
    if new.email() == other.email() {
        return Some(keys::INVITE_EMAIL);
    }
    (new.token_hash() == other.token_hash()).then_some(keys::INVITE_TOKEN)
}

impl InviteRepo for InMemoryInviteRepo {
    async fn get_by_email(&self, email: &Email) -> Result<Option<Invite>, RepoError> {
        Ok(self.invites.find(|i| i.email() == email))
    }

    async fn get_by_token(&self, hash: TokenHash) -> Result<Option<Invite>, RepoError> {
        Ok(self.invites.find(|i| i.token_hash() == hash))
    }

    async fn insert(&self, invite: &mut Invite) -> Result<(), RepoError> {
        self.invites
            .insert(invite.id(), invite, keys::INVITE_ID, &unique_invite)
    }

    async fn update(&self, invite: &mut Invite) -> Result<(), RepoError> {
        self.invites.update(invite.id(), invite, &unique_invite)
    }
}
