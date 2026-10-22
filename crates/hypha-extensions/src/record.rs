//! The call log (`extension_calls`): one row per call Hypha made to an app's extension
//! point, shown to staff per app and pruned by age.

use core::time::Duration;

use hypha_core::{
    AppKey, Clock, Ctx, ExtensionCallId, Page, PageRequest, Progress, RepoError, Scope, Timestamp,
};

use crate::CommandError;

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallOutcome {
    /// A 2xx with a body that is the point's response.
    Ok,
    /// No answer (refused, unreachable, broken off) or a status other than 2xx.
    Failed,
    /// No answer within the point's (or the app's) timeout.
    Timeout,
    /// A 2xx whose body isn't the point's response, breaks its rules, or is too big.
    Invalid,
}

impl CallOutcome {
    pub const ALL: [CallOutcome; 4] = [
        CallOutcome::Ok,
        CallOutcome::Failed,
        CallOutcome::Timeout,
        CallOutcome::Invalid,
    ];

    /// As the database and the API spell it.
    pub fn as_str(self) -> &'static str {
        match self {
            CallOutcome::Ok => "ok",
            CallOutcome::Failed => "failed",
            CallOutcome::Timeout => "timeout",
            CallOutcome::Invalid => "invalid",
        }
    }

    /// The outcome `as_str` spells, or the text that isn't one.
    pub fn parse(s: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|o| o.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

/// One call to an app's extension point, as it ended. A record, not an aggregate: it's
/// written once and never changed, so it has no version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionCall {
    pub id: ExtensionCallId,
    pub app: AppKey,
    /// The point's `NAME` (`shipping.rates`); text, so a log keeps points a later build
    /// retired.
    pub point: String,
    pub version: u16,
    pub started_at: Timestamp,
    pub duration_ms: u32,
    pub outcome: CallOutcome,
    /// The answer's HTTP status, when there was an answer.
    pub status: Option<u16>,
    /// Why it wasn't `Ok`, in words the merchant may see: transport details stay in the
    /// host's log (`outbound.rs`), an app's own error `code` is kept.
    pub error: Option<String>,
    /// What the call was for: the prefixed ID of the cart, order or payment
    /// (`cart_…`), to find a flow's calls.
    pub correlation: Option<String>,
}

#[allow(async_fn_in_trait)]
pub trait ExtensionCallRepo {
    /// Stores a call. IDs are new for every call; a duplicate is the caller's bug and
    /// fails with `Duplicate(keys::EXTENSION_CALL_ID)`.
    async fn insert(&self, call: &ExtensionCall) -> Result<(), RepoError>;
    /// An app's calls, newest first (by ID: UUIDv7, so by start).
    async fn list_for_app(
        &self,
        app: &AppKey,
        page: PageRequest<ExtensionCallId>,
    ) -> Result<Page<ExtensionCall>, RepoError>;
    /// Deletes up to `limit` calls that started before `before`; how many it deleted.
    async fn prune(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError>;
}

/// Uniqueness rules the call log enforces, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const EXTENSION_CALL_ID: &str = "extension_call_id";
}

/// Whatever holds the `ExtensionCallRepo`: a host's per-request ports, a test world.
pub trait HasExtensionCalls {
    type ExtensionCalls: ExtensionCallRepo;
    fn extension_calls(&self) -> &Self::ExtensionCalls;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasExtensionCalls + ?Sized> HasExtensionCalls for &T {
    type ExtensionCalls = T::ExtensionCalls;
    fn extension_calls(&self) -> &Self::ExtensionCalls {
        (**self).extension_calls()
    }
}

/// How long the call log is kept (`jobs.prune.extension_calls_after`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionSettings {
    pub keep_calls: Duration,
}

impl Default for ExtensionSettings {
    fn default() -> Self {
        Self {
            // As long as events and their deliveries.
            keep_calls: Duration::from_secs(30 * 24 * 3600),
        }
    }
}

/// An app's calls, newest first. Under `apps:read`, like its deliveries. Doesn't check
/// that the app exists (apps are another leaf domain): a route loads the app first.
pub async fn list_extension_calls<S: HasExtensionCalls>(
    ctx: &Ctx,
    ports: &S,
    app: &AppKey,
    page: PageRequest<ExtensionCallId>,
) -> Result<Page<ExtensionCall>, CommandError> {
    ctx.require(Scope::AppsRead)?;
    Ok(ports.extension_calls().list_for_app(app, page).await?)
}

/// Deletes calls older than `keep_calls`, up to `batch` of them: the prune job's step.
pub async fn prune_calls<S: HasExtensionCalls>(
    ports: &S,
    clock: &impl Clock,
    settings: &ExtensionSettings,
    batch: u32,
) -> Result<Progress, RepoError> {
    let before = clock.now().minus(settings.keep_calls);
    let done = ports.extension_calls().prune(before, batch).await?;
    Ok(Progress::of(done, batch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_round_trip_through_their_names() {
        for outcome in CallOutcome::ALL {
            assert_eq!(CallOutcome::parse(outcome.as_str()), Ok(outcome));
        }
        assert_eq!(CallOutcome::parse("lost"), Err("lost".to_string()));
    }
}
