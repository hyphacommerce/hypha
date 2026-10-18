//! Staff users, roles and invites (design note 0008), and the staff flows built on
//! `hypha-auth`: the owner's first start, invites, login and password reset.
//!
//! A composing domain: it depends on `hypha-auth` and core.

pub mod commands;
pub mod error;
pub mod invite;
pub mod repo;
pub mod role;
pub mod user;

pub use commands::{
    AcceptInvite, CommandError, InviteDetails, InviteUser, NewOwner, StaffEmail, StaffPorts,
    StaffSession, UpdateRole, UpdateUser, accept_invite, authenticate_staff, bootstrap_owner,
    create_role, get_role, get_user, invite_user, list_invites, list_roles, list_users, login,
    lookup_invite, rename_me, request_password_reset, revoke_invite, update_role, update_user,
};
pub use error::{RoleError, UserError};
pub use invite::{Invite, InviteSnapshot};
pub use repo::{
    HasInvites, HasRoles, HasStaffSessions, HasUsers, InviteRepo, RoleRepo, StaffSessionRecord,
    StaffSessionRepo, StageInvites, StageRoles, StageUsers, UserRepo, keys,
};
pub use role::{Role, RoleScopes, RoleSnapshot};
pub use user::{User, UserSnapshot, UserStatus};
