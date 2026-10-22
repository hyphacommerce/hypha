//! Staff users: the people who run the store. How they log in lives in their
//! `hypha_auth::Identity`.

use hypha_core::{Email, RoleId, Timestamp, UserId, Version, Versioned, required_text};

use crate::error::UserError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    Active,
    /// Can't log in; their sessions stop working on the next request.
    Disabled,
}

impl UserStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            UserStatus::Active => "active",
            UserStatus::Disabled => "disabled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(UserStatus::Active),
            "disabled" => Some(UserStatus::Disabled),
            _ => None,
        }
    }
}

impl std::fmt::Display for UserStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    id: UserId,
    version: Version,
    email: Email,
    name: String,
    role: RoleId,
    status: UserStatus,
    created_at: Timestamp,
}

impl Versioned for User {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl User {
    /// An active user. The name is trimmed and required.
    pub fn new(
        id: UserId,
        email: Email,
        name: &str,
        role: RoleId,
        created_at: Timestamp,
    ) -> Result<Self, UserError> {
        Ok(Self {
            id,
            version: Version::NEW,
            email,
            name: required_text("name", name)?,
            role,
            status: UserStatus::Active,
            created_at,
        })
    }

    pub fn id(&self) -> UserId {
        self.id
    }

    pub fn email(&self) -> &Email {
        &self.email
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn role(&self) -> RoleId {
        self.role
    }

    pub fn status(&self) -> UserStatus {
        self.status
    }

    pub fn is_active(&self) -> bool {
        self.status == UserStatus::Active
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn rename(&mut self, name: &str) -> Result<(), UserError> {
        self.name = required_text("name", name)?;
        Ok(())
    }

    pub fn set_role(&mut self, role: RoleId) {
        self.role = role;
    }

    pub fn set_status(&mut self, status: UserStatus) {
        self.status = status;
    }

    pub fn snapshot(&self) -> UserSnapshot {
        UserSnapshot {
            id: self.id,
            version: self.version,
            email: self.email.clone(),
            name: self.name.clone(),
            role: self.role,
            status: self.status,
            created_at: self.created_at,
        }
    }

    pub fn restore(s: UserSnapshot) -> Result<User, UserError> {
        let mut user = User::new(s.id, s.email, &s.name, s.role, s.created_at)?;
        user.version = s.version;
        user.status = s.status;
        Ok(user)
    }
}

/// Plain-data form of a `User` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSnapshot {
    pub id: UserId,
    pub version: Version,
    pub email: Email,
    pub name: String,
    pub role: RoleId,
    pub status: UserStatus,
    pub created_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    fn user(name: &str) -> Result<User, UserError> {
        User::new(
            UserId::from_uuid(Uuid::from_u128(1)),
            Email::new("ana@example.com").unwrap(),
            name,
            RoleId::from_uuid(Uuid::from_u128(2)),
            Timestamp::UNIX_EPOCH,
        )
    }

    #[test]
    fn a_new_user_is_active_with_a_trimmed_name() {
        let u = user(" Ana ").unwrap();
        assert_eq!(u.name(), "Ana");
        assert!(u.is_active());
    }

    #[test]
    fn a_user_needs_a_name() {
        assert_eq!(user(""), Err(UserError::Text(TextError::Missing("name"))));
    }

    #[test]
    fn snapshot_round_trips() {
        let mut u = user("Ana").unwrap();
        u.set_status(UserStatus::Disabled);
        assert_eq!(User::restore(u.snapshot()), Ok(u));
    }
}
