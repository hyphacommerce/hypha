//! Staff settings: set from `hypha.config.toml`'s `[auth]` by the host, empty by default.

use hypha_core::Email;

/// Accounts locked against changing themselves: a public demo's shared login, which
/// every visitor signs in with. They keep what their role allows, but can't change
/// their name, password, two-factor setup or anyone else's session, see only their own
/// session, and get no reset emails: one visitor can't lock the others out, or watch
/// where they sign in from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaffSettings {
    pub locked_accounts: Vec<Email>,
}

impl StaffSettings {
    pub fn is_locked(&self, email: &Email) -> bool {
        self.locked_accounts.contains(email)
    }
}
