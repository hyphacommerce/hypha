//! Roles: named sets of scopes (design note 0008, A6).

use hypha_core::{RoleId, Scope, Version, Versioned, required_text};

use crate::error::RoleError;

/// What a role grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleScopes {
    /// Every scope, including ones added in later versions: the owner role.
    All,
    /// These scopes, in `Scope::ALL` order without repeats.
    Only(Vec<Scope>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    id: RoleId,
    version: Version,
    name: String,
    scopes: RoleScopes,
}

impl Versioned for Role {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Role {
    pub const OWNER_NAME: &'static str = "Owner";

    /// The built-in owner role. One per store; it can't be edited.
    pub fn owner(id: RoleId) -> Self {
        Self {
            id,
            version: Version::NEW,
            name: Self::OWNER_NAME.to_string(),
            scopes: RoleScopes::All,
        }
    }

    /// A role granting `scopes` (at least one). The name is trimmed and required.
    pub fn new(id: RoleId, name: &str, scopes: &[Scope]) -> Result<Self, RoleError> {
        Ok(Self {
            id,
            version: Version::NEW,
            name: required_text("name", name)?,
            scopes: RoleScopes::Only(valid_scopes(scopes)?),
        })
    }

    pub fn id(&self) -> RoleId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn scopes(&self) -> &RoleScopes {
        &self.scopes
    }

    pub fn is_owner(&self) -> bool {
        self.scopes == RoleScopes::All
    }

    /// The scopes a member gets, `All` spelled out.
    pub fn granted(&self) -> Vec<Scope> {
        match &self.scopes {
            RoleScopes::All => Scope::ALL.to_vec(),
            RoleScopes::Only(scopes) => scopes.clone(),
        }
    }

    pub fn rename(&mut self, name: &str) -> Result<(), RoleError> {
        if self.is_owner() {
            return Err(RoleError::OwnerRoleFixed);
        }
        self.name = required_text("name", name)?;
        Ok(())
    }

    pub fn set_scopes(&mut self, scopes: &[Scope]) -> Result<(), RoleError> {
        if self.is_owner() {
            return Err(RoleError::OwnerRoleFixed);
        }
        self.scopes = RoleScopes::Only(valid_scopes(scopes)?);
        Ok(())
    }

    pub fn snapshot(&self) -> RoleSnapshot {
        RoleSnapshot {
            id: self.id,
            version: self.version,
            name: self.name.clone(),
            scopes: self.scopes.clone(),
        }
    }

    /// Rebuilds a role from storage, re-checking the rules.
    pub fn restore(s: RoleSnapshot) -> Result<Role, RoleError> {
        let mut role = match &s.scopes {
            RoleScopes::All if s.name == Self::OWNER_NAME => Role::owner(s.id),
            RoleScopes::All => return Err(RoleError::OwnerRoleFixed),
            RoleScopes::Only(scopes) => Role::new(s.id, &s.name, scopes)?,
        };
        role.version = s.version;
        Ok(role)
    }
}

fn valid_scopes(scopes: &[Scope]) -> Result<Vec<Scope>, RoleError> {
    let scopes: Vec<Scope> = Scope::ALL
        .into_iter()
        .filter(|s| scopes.contains(s))
        .collect();
    if scopes.is_empty() {
        return Err(RoleError::NoScopes);
    }
    Ok(scopes)
}

/// Plain-data form of a `Role` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleSnapshot {
    pub id: RoleId,
    pub version: Version,
    pub name: String,
    pub scopes: RoleScopes,
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    fn rid() -> RoleId {
        RoleId::from_uuid(Uuid::from_u128(1))
    }

    #[test]
    fn a_role_keeps_its_scopes_sorted_and_once() {
        let role = Role::new(
            rid(),
            " Support ",
            &[Scope::OrdersRead, Scope::CustomersRead, Scope::OrdersRead],
        )
        .unwrap();
        assert_eq!(role.name(), "Support");
        assert_eq!(role.granted(), [Scope::CustomersRead, Scope::OrdersRead]);
        assert!(!role.is_owner());
    }

    #[test]
    fn a_role_needs_a_name_and_a_scope() {
        assert_eq!(
            Role::new(rid(), " ", &[Scope::OrdersRead]),
            Err(RoleError::Text(TextError::Missing("name")))
        );
        assert_eq!(Role::new(rid(), "Empty", &[]), Err(RoleError::NoScopes));
    }

    #[test]
    fn the_owner_has_every_scope_and_cannot_change() {
        let mut owner = Role::owner(rid());
        assert_eq!(owner.granted(), Scope::ALL);
        assert_eq!(owner.rename("Boss"), Err(RoleError::OwnerRoleFixed));
        assert_eq!(
            owner.set_scopes(&[Scope::OrdersRead]),
            Err(RoleError::OwnerRoleFixed)
        );
    }

    #[test]
    fn snapshot_round_trips() {
        for role in [
            Role::owner(rid()),
            Role::new(rid(), "Support", &[Scope::OrdersRead]).unwrap(),
        ] {
            assert_eq!(Role::restore(role.snapshot()), Ok(role));
        }
    }
}
