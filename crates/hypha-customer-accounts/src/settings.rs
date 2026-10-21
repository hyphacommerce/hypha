//! How long a customer's code and session last (design note 0034, AC5): set from
//! `hypha.config.toml`'s `[customers]` by the host, defaults here.

use core::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountSettings {
    /// How long a customer stays signed in without using the session; every use within
    /// it pushes the end out again (`Session::extend`).
    pub session_lifetime: Duration,
    /// How long an emailed code works.
    pub login_code_lifetime: Duration,
    /// While the latest code is younger than this, asking again sends nothing: at most
    /// one email per address every so often, however often it's asked.
    pub login_code_resend_after: Duration,
}

impl Default for AccountSettings {
    fn default() -> Self {
        Self {
            session_lifetime: 30 * DAY,
            login_code_lifetime: 10 * MINUTE,
            login_code_resend_after: MINUTE,
        }
    }
}

/// Where the commands read `AccountSettings` from: part of `AccountEnv`.
pub trait HasAccountSettings {
    fn account_settings(&self) -> &AccountSettings;
}
