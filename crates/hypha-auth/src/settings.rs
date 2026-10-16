//! How long auth's tokens last (design note 0017): set from `hypha.config.toml`'s `[auth]`
//! by the host, defaults here.

use core::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Auth's lifetimes. Plain data: hosts build it from their configuration, tests use the
/// defaults or set one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthSettings {
    /// How long an active session lasts, from login.
    pub session_lifetime: Duration,
    /// How long a pending session waits for the second factor.
    pub second_factor_window: Duration,
    /// How long second-factor checks stop after `Identity::MAX_MFA_FAILURES` wrong codes.
    pub second_factor_lock: Duration,
    /// How long an emailed password reset link works.
    pub password_reset_lifetime: Duration,
    /// While the latest reset link is younger than this and unused, no new one is sent:
    /// at most one reset email per account every few minutes, however often it's asked.
    pub password_reset_resend_after: Duration,
    /// How long an invite works. Invites are `hypha-users`', but their token is one auth
    /// issues, and their commands take the same `AuthEnv`.
    pub invite_lifetime: Duration,
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            session_lifetime: 7 * DAY,
            second_factor_window: 5 * MINUTE,
            second_factor_lock: 15 * MINUTE,
            password_reset_lifetime: 60 * MINUTE,
            password_reset_resend_after: 5 * MINUTE,
            invite_lifetime: 7 * DAY,
        }
    }
}

/// Where commands read `AuthSettings` from: part of `AuthEnv`.
pub trait HasAuthSettings {
    fn auth_settings(&self) -> &AuthSettings;
}
