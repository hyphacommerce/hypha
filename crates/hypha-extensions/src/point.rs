//! What an extension point is (design note 0029, AP3): a published, versioned contract
//! with a timeout and a failure policy.

use core::time::Duration;

use serde::{Serialize, de::DeserializeOwned};

use crate::ExtensionError;

/// A synchronous contract Hypha calls apps through. One impl per point and version: a
/// reshaped request or response is a new version, never an edit of this one.
///
/// The request and response types are the wire format apps are written against, and
/// documented in `openapi/extensions.json` (with the `openapi` feature they implement
/// `utoipa::ToSchema`; the bound isn't here so builds without the feature stay lean).
pub trait ExtensionPoint {
    /// Dotted: `payments.start`. Sent as `Hypha-Extension`.
    const NAME: &'static str;
    /// Sent as `Hypha-Extension-Version`.
    const VERSION: u16;
    /// How long Hypha waits for an answer unless the app's registration sets its own
    /// (a shipping provider's `timeout_ms`).
    const TIMEOUT: Duration;
    const ON_FAILURE: OnFailure;
    /// The body Hypha sends.
    type Request: Serialize;
    /// The body a 2xx answer must parse as; anything else is an `invalid` call.
    type Response: DeserializeOwned;

    /// Rules on an answer that its type can't say (amounts not negative, the request's
    /// currency): broken, the call is `invalid`, as if it hadn't parsed. The reason
    /// is recorded on the call, so it's what the merchant sees.
    fn check(request: &Self::Request, response: &Self::Response) -> Result<(), String> {
        let _ = (request, response);
        Ok(())
    }
}

/// What a caller does when a point's app fails (no answer, a non-2xx, a malformed one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnFailure {
    /// The flow stops with the typed error (`app_unavailable`): a payment that can't
    /// start isn't placed.
    FailClosed,
    /// Core's default is used instead (search falls back to the built-in engine,
    /// shipping hides the app's options), and the failure is logged.
    Fallback,
}

impl OnFailure {
    /// As the extensions document names it.
    pub fn as_str(self) -> &'static str {
        match self {
            OnFailure::FailClosed => "fail_closed",
            OnFailure::Fallback => "fallback",
        }
    }
}

/// A call's result under its point's policy (`apply_policy`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answered<T> {
    /// The app answered.
    App(T),
    /// The app failed and the point falls back: the caller uses core's default. The
    /// error says why, for the caller's log (the call log has it already).
    Fallback(ExtensionError),
}

/// `call`'s result under `P::ON_FAILURE`: a failure of a `Fallback` point becomes
/// `Answered::Fallback`, one of a `FailClosed` point stays the error. `call` itself
/// never applies the policy, so a caller that knows better (checkout re-quoting a
/// rate) can still see the error.
pub fn apply_policy<P: ExtensionPoint>(
    result: Result<P::Response, ExtensionError>,
) -> Result<Answered<P::Response>, ExtensionError> {
    match (result, P::ON_FAILURE) {
        (Ok(response), _) => Ok(Answered::App(response)),
        (Err(e), OnFailure::Fallback) => Ok(Answered::Fallback(e)),
        (Err(e), OnFailure::FailClosed) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::AppKey;

    use super::*;

    struct Closed;
    impl ExtensionPoint for Closed {
        const NAME: &'static str = "test.closed";
        const VERSION: u16 = 1;
        const TIMEOUT: Duration = Duration::from_secs(1);
        const ON_FAILURE: OnFailure = OnFailure::FailClosed;
        type Request = ();
        type Response = u8;
    }

    struct Open;
    impl ExtensionPoint for Open {
        const NAME: &'static str = "test.open";
        const VERSION: u16 = 1;
        const TIMEOUT: Duration = Duration::from_secs(1);
        const ON_FAILURE: OnFailure = OnFailure::Fallback;
        type Request = ();
        type Response = u8;
    }

    fn timeout(point: &'static str) -> ExtensionError {
        ExtensionError::Timeout {
            app: AppKey::new("app_x").unwrap(),
            point,
        }
    }

    #[test]
    fn an_answer_is_the_apps_under_either_policy() {
        assert_eq!(apply_policy::<Closed>(Ok(7)), Ok(Answered::App(7)));
        assert_eq!(apply_policy::<Open>(Ok(7)), Ok(Answered::App(7)));
    }

    #[test]
    fn a_fail_closed_point_keeps_the_error() {
        assert_eq!(
            apply_policy::<Closed>(Err(timeout(Closed::NAME))),
            Err(timeout(Closed::NAME))
        );
    }

    #[test]
    fn a_fallback_point_turns_the_error_into_a_fallback() {
        assert_eq!(
            apply_policy::<Open>(Err(timeout(Open::NAME))),
            Ok(Answered::Fallback(timeout(Open::NAME)))
        );
    }

    #[test]
    fn policies_have_stable_names() {
        assert_eq!(OnFailure::FailClosed.as_str(), "fail_closed");
        assert_eq!(OnFailure::Fallback.as_str(), "fallback");
    }
}
