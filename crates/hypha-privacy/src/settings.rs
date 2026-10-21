use std::time::Duration;

/// The privacy workflows' tunables (design note 0032, §7.3), set by `hypha.config.toml`'s
/// `[customers]` table and passed in like a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivacySettings {
    /// How long a requested erasure can be cancelled before the job runs it
    /// (`customers.erasure_delay`). A merchant under a retention duty sets it longer.
    pub erasure_delay: Duration,
}

impl PrivacySettings {
    /// The shortest window: an hour, so a mistaken click can always be taken back.
    pub const MIN_ERASURE_DELAY: Duration = Duration::from_secs(3600);
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            erasure_delay: Duration::from_secs(10 * 24 * 3600),
        }
    }
}
